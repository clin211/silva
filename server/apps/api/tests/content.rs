//! 文章、审核与站点发布的验收测试：真实 PostgreSQL、内存对象存储。
//!
//! 构建器默认按快照生成最小页面；设置 `SILVA_TEST_WEBSITE_DIR`（指向 `frontend/apps/website`，
//! 需要本机 Node 与 pnpm）时改用真实的 Astro 构建。
//! 通过 `cargo test -p silva-api --test content -- --ignored` 显式执行。
#[path = "support/storage.rs"]
mod storage;
mod support;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use silva_application::{
    ports::{
        BuiltFile, ContentActor, ContentRepository, NewRevision, RenderedMarkdown, RequestMeta,
        SiteBuilder,
    },
    publishing::PublishOutcome,
};
use silva_domain::content::RevisionContent;
use silva_domain::site::SiteSnapshot;
use silva_shared::{
    config::{Config, JwtSecret},
    error::{AppError, Result as AppResult},
    ids,
};
use sqlx::{Connection, PgConnection, PgPool, postgres::PgConnectOptions};
use std::{
    io::Cursor,
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use storage::MemoryStorage;
use tower::ServiceExt;

const PASSWORD: &str = "acceptance-password-2026";
const JWT_SECRET: &[u8] = b"acceptance-jwt-secret-0123456789abcdef";

/// 按快照生成最小站点的构建器，用于无 Node 环境；可切换为失败以验证重试。
struct SnapshotPages;

#[async_trait]
impl SiteBuilder for SnapshotPages {
    async fn build(&self, snapshot: &[u8], _: &str, generation: i64) -> AppResult<Vec<BuiltFile>> {
        let snapshot: SiteSnapshot = serde_json::from_slice(snapshot).unwrap();
        assert_eq!(snapshot.generation, generation);
        let page = |path: String, body: String| BuiltFile {
            path,
            content_type: "text/html; charset=utf-8".into(),
            bytes: format!("<meta name=\"silva:generation\" content=\"{generation}\">{body}")
                .into_bytes(),
        };
        let mut files = vec![
            page(
                "index.html".into(),
                format!("posts:{}", snapshot.posts.len()),
            ),
            page("404.html".into(), "not found".into()),
            BuiltFile {
                path: "_astro/site.css".into(),
                content_type: "text/css; charset=utf-8".into(),
                bytes: b"body{}".to_vec(),
            },
        ];
        for post in &snapshot.posts {
            files.push(page(
                format!("blog/{}/index.html", post.slug),
                post.html.clone(),
            ));
        }
        Ok(files)
    }
}

/// 包装真实或替身构建器，`fail` 打开时直接失败。
struct Switchable {
    inner: Arc<dyn SiteBuilder>,
    fail: AtomicBool,
}

#[async_trait]
impl SiteBuilder for Switchable {
    async fn build(
        &self,
        snapshot: &[u8],
        sha256: &str,
        generation: i64,
    ) -> AppResult<Vec<BuiltFile>> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(AppError::Internal(anyhow::anyhow!(
                "simulated build failure"
            )));
        }
        self.inner.build(snapshot, sha256, generation).await
    }
}

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
    json: Value,
}

impl Response {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    fn generation(&self) -> Option<i64> {
        self.headers
            .get("x-site-generation")
            .map(|value| value.to_str().unwrap().parse().unwrap())
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
    extra: &[(&str, &str)],
) -> Response {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::USER_AGENT, "silva-content-acceptance/1.0")
        .extension(ConnectInfo(
            "192.0.2.20:40000".parse::<SocketAddr>().unwrap(),
        ));
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    if path.starts_with("/api/") {
        support::assert_response(&method, path, status, &headers, &bytes);
    }
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.to_str().unwrap().starts_with("application/json"));
    let json = if is_json {
        serde_json::from_slice(&bytes).unwrap()
    } else {
        Value::Null
    };
    Response {
        status,
        headers,
        bytes,
        json,
    }
}

async fn api(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: &str,
) -> Response {
    call(app, method, path, body, Some(token), &[]).await
}

async fn get(app: &Router, path: &str, token: &str) -> Response {
    api(app, Method::GET, path, None, token).await
}

fn assert_error(response: &Response, status: StatusCode, code: &str) {
    assert_eq!(response.status, status, "{}", response.json);
    assert_eq!(response.json["error"]["code"], code, "{}", response.json);
}

fn assert_field(response: &Response, field: &str, code: &str) {
    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    );
    assert!(
        response.json["error"]["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|detail| detail["field"] == field && detail["code"] == code),
        "缺少 {field}/{code}: {}",
        response.json
    );
}

async fn login(app: &Router, email: &str) -> String {
    let response = call(
        app,
        Method::POST,
        "/api/v1/admin/sessions",
        Some(json!({"email": email, "password": PASSWORD})),
        None,
        &[],
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    response.json["data"]["access_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn config(database_url: String, site_serve: bool) -> Config {
    Config {
        database_url,
        bind: "127.0.0.1:0".parse().unwrap(),
        production: false,
        jwt_secret: Some(JwtSecret::new(JWT_SECRET)),
        db_max_connections: 5,
        access_token_ttl: Duration::from_secs(900),
        refresh_token_ttl: Duration::from_secs(3600),
        worker_interval: Duration::from_secs(60),
        api_rate_per_minute: 10_000,
        login_rate_per_minute: 1000,
        login_lockout_threshold: 1000,
        login_lockout_duration: Duration::from_secs(900),
        storage: None,
        media_upload_ttl: Duration::from_secs(900),
        site_build: None,
        publish_poll_interval: Duration::from_secs(5),
        site_serve,
    }
}

/// 从文章路径 `/api/v1/admin/posts/{id}` 取出文章 ID。
fn current_post_id(path: &str) -> uuid::Uuid {
    path.rsplit('/').next().unwrap().parse().unwrap()
}

fn png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        32,
        18,
        image::Rgb([10, 90, 160]),
    ));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// 上传并确认一张图片，返回媒体 ID。
async fn ready_media(app: &Router, storage: &MemoryStorage, token: &str) -> String {
    let bytes = png();
    let created = api(
        app,
        Method::POST,
        "/api/v1/admin/media",
        Some(json!({"filename": "cover.png", "declared_mime_type": "image/png", "size_bytes": bytes.len()})),
        token,
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let id = created.json["data"]["media"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(storage.upload(
        created.json["data"]["upload"]["url"].as_str().unwrap(),
        &bytes
    ));
    let confirmed = api(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/media/{id}"),
        Some(json!({"status": "uploaded", "alt_text": "封面"})),
        token,
    )
    .await;
    assert_eq!(confirmed.json["data"]["status"], "ready");
    id
}

#[tokio::test]
#[ignore = "需要通过 SILVA_TEST_DATABASE_URL 连接 PostgreSQL，且账户具备 CREATEDB 权限"]
async fn content_review_publishing_and_public_site() {
    let database_url = std::env::var("SILVA_TEST_DATABASE_URL")
        .expect("set SILVA_TEST_DATABASE_URL; real PostgreSQL tests never silently skip");
    let options = PgConnectOptions::from_str(&database_url).unwrap();
    let mut admin = PgConnection::connect_with(&options).await.unwrap();
    let database_name = format!("silva_content_{}", ids::new_id().simple());
    sqlx::query(&format!("CREATE DATABASE \"{database_name}\""))
        .execute(&mut admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(options.database(&database_name))
        .await
        .unwrap();
    let schema = match silva_infrastructure::persistence::migrate(&pool).await {
        Ok(()) => silva_infrastructure::persistence::check_schema(&pool).await,
        Err(error) => Err(error),
    };
    let test_pool = pool.clone();
    let result = match &schema {
        Ok(()) => Some(tokio::spawn(async move { run(&test_pool, database_url).await }).await),
        Err(_) => None,
    };
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database_name}\" WITH (FORCE)"))
        .execute(&mut admin)
        .await
        .unwrap();
    schema.expect("migrations or schema invariants failed");
    result
        .unwrap()
        .expect("content acceptance assertions failed");
}

async fn run(pool: &PgPool, database_url: String) {
    let tokens = silva_infrastructure::TokenSettings {
        secret: JwtSecret::new(JWT_SECRET),
        access_ttl: Duration::from_secs(900),
        refresh_ttl: Duration::from_secs(3600),
    };
    let identity = silva_infrastructure::identity_service(pool.clone(), &tokens)
        .await
        .unwrap();
    for (email, role) in [
        ("admin@example.test", "admin"),
        ("author@example.test", "author"),
        ("other@example.test", "author"),
        ("editor@example.test", "editor"),
    ] {
        identity
            .provision_user(
                email.into(),
                role.into(),
                PASSWORD.into(),
                vec![role.into()],
            )
            .await
            .unwrap();
    }
    let storage = Arc::new(MemoryStorage::default());
    let app = silva_api::build_router_with_storage(
        config(database_url.clone(), false),
        pool.clone(),
        Some(storage.clone()),
    )
    .await
    .unwrap();
    let site_app = silva_api::build_router_with_storage(
        config(database_url.clone(), true),
        pool.clone(),
        Some(storage.clone()),
    )
    .await
    .unwrap();
    let inner: Arc<dyn SiteBuilder> = match std::env::var("SILVA_TEST_WEBSITE_DIR") {
        Ok(dir) => Arc::new(silva_infrastructure::site_builder::AstroSiteBuilder {
            website_dir: dir.into(),
            command: vec!["pnpm".into(), "run".into(), "build".into()],
            site_url: "https://silva.example".into(),
            timeout: Duration::from_secs(600),
        }),
        Err(_) => Arc::new(SnapshotPages),
    };
    let real_astro = std::env::var("SILVA_TEST_WEBSITE_DIR").is_ok();
    let builder = Arc::new(Switchable {
        inner,
        fail: AtomicBool::new(false),
    });
    let worker = silva_infrastructure::publishing_service(
        pool.clone(),
        Some(storage.clone()),
        Some(builder.clone()),
    );

    let admin = login(&app, "admin@example.test").await;
    let author = login(&app, "author@example.test").await;
    let other = login(&app, "other@example.test").await;
    let editor = login(&app, "editor@example.test").await;

    // 分类与标签：编辑维护，作者只读；slug 唯一且格式受限。
    let category = api(
        &app,
        Method::POST,
        "/api/v1/admin/categories",
        Some(json!({"slug": "backend", "name": "后端", "description": "服务端实践"})),
        &editor,
    )
    .await;
    assert_eq!(category.status, StatusCode::CREATED, "{}", category.json);
    let category_id = category.json["data"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        category.headers[header::LOCATION],
        format!("/api/v1/admin/categories/{category_id}").as_str()
    );
    let tag = api(
        &app,
        Method::POST,
        "/api/v1/admin/tags",
        Some(json!({"slug": "rust", "name": "Rust"})),
        &editor,
    )
    .await;
    let tag_id = tag.json["data"]["id"].as_str().unwrap().to_owned();
    let spare_tag = api(
        &app,
        Method::POST,
        "/api/v1/admin/tags",
        Some(json!({"slug": "spare", "name": "备用"})),
        &editor,
    )
    .await;
    let spare_tag_id = spare_tag.json["data"]["id"].as_str().unwrap().to_owned();
    assert_error(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/tags",
            Some(json!({"slug": "rust", "name": "重复"})),
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "slug_in_use",
    );
    assert_field(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/tags",
            Some(json!({"slug": "Bad Slug", "name": "x"})),
            &editor,
        )
        .await,
        "slug",
        "invalid_format",
    );
    assert_error(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/tags",
            Some(json!({"slug": "x", "name": "x"})),
            &author,
        )
        .await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );
    let listed = get(&app, "/api/v1/admin/categories", &author).await;
    assert_eq!(listed.json["data"][0]["slug"], "backend");

    // 创建文章：正文图片引用受控媒体，原始 HTML 被转义。
    let cover = ready_media(&app, &storage, &author).await;
    let others_media = ready_media(&app, &storage, &other).await;
    let body = format!(
        "## 介绍\n\n正文 <script>alert(1)</script>\n\n![图](media:{cover})\n\n```rust\nfn main() {{}}\n```\n"
    );
    let post_body = |slug: &str| {
        json!({
            "slug": slug, "title": "用 Axum 构建服务", "excerpt": "一篇示例", "markdown_body": body,
            "category_ids": [category_id], "tag_ids": [tag_id], "cover_media_id": cover, "is_featured": true
        })
    };
    let created = api(
        &app,
        Method::POST,
        "/api/v1/admin/posts",
        Some(post_body("building-with-axum")),
        &author,
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let post = &created.json["data"];
    let post_id = post["id"].as_str().unwrap().to_owned();
    let posts_path = format!("/api/v1/admin/posts/{post_id}");
    assert_eq!(created.headers[header::LOCATION], posts_path.as_str());
    let revision = &post["current_revision"];
    assert_eq!(revision["revision_no"], 1);
    let html = revision["rendered_html"].as_str().unwrap();
    assert!(
        html.contains(&format!(
            r#"src="/api/v1/public/media/{cover}" width="32" height="18""#
        )),
        "{html}"
    );
    assert!(html.contains("&lt;script&gt;") && !html.contains("<script"));
    assert_eq!(revision["headings"][0]["slug"], "介绍");
    assert_eq!(revision["media_ids"], json!([cover]));
    assert_eq!(revision["categories"][0]["slug"], "backend");
    assert_eq!(post["publication"]["publication_version"], 0);
    assert!(post["publication"]["live_revision_id"].is_null());

    assert_error(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/posts",
            Some(post_body("building-with-axum")),
            &author,
        )
        .await,
        StatusCode::CONFLICT,
        "slug_in_use",
    );
    let mut invalid = post_body("bad-image");
    invalid["markdown_body"] = json!("![x](https://example.com/a.png)");
    assert_field(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/posts",
            Some(invalid),
            &author,
        )
        .await,
        "markdown_body",
        "invalid_value",
    );
    let mut invalid = post_body("bad-category");
    invalid["category_ids"] = json!([ids::new_id()]);
    assert_field(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/posts",
            Some(invalid),
            &author,
        )
        .await,
        "category_ids[0]",
        "invalid_value",
    );
    let mut invalid = post_body("not-mine");
    invalid["cover_media_id"] = json!(others_media);
    assert_field(
        &api(
            &app,
            Method::POST,
            "/api/v1/admin/posts",
            Some(invalid),
            &author,
        )
        .await,
        "cover_media_id",
        "invalid_value",
    );

    // 归属：其他作者看不到；编辑可以看到全部。
    assert_error(
        &get(&app, &posts_path, &other).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    assert_eq!(
        get(&app, "/api/v1/admin/posts", &other).await.json["meta"]["pagination"]["total"],
        0
    );
    assert_eq!(
        get(&app, "/api/v1/admin/posts?published=false&q=axum", &editor)
            .await
            .json["meta"]["pagination"]["total"],
        1
    );

    // 保存修订：有变化创建新修订，内容相同返回当前版本，过期的 expected_revision_id 冲突。
    let revision_one = revision["id"].as_str().unwrap().to_owned();
    let revision_body = |expected: &str, title: &str| {
        json!({
            "expected_revision_id": expected, "title": title, "excerpt": "一篇示例", "markdown_body": body,
            "category_ids": [category_id], "tag_ids": [tag_id], "cover_media_id": cover, "is_featured": true
        })
    };
    let saved = api(
        &app,
        Method::POST,
        &format!("{posts_path}/revisions"),
        Some(revision_body(&revision_one, "第二版")),
        &author,
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.json);
    let revision_two = saved.json["data"]["id"].as_str().unwrap().to_owned();
    assert_eq!(saved.json["data"]["revision_no"], 2);
    let unchanged = api(
        &app,
        Method::POST,
        &format!("{posts_path}/revisions"),
        Some(revision_body(&revision_two, "第二版")),
        &author,
    )
    .await;
    assert_eq!(unchanged.status, StatusCode::OK);
    assert_eq!(unchanged.json["data"]["id"], revision_two);
    assert_error(
        &api(
            &app,
            Method::POST,
            &format!("{posts_path}/revisions"),
            Some(revision_body(&revision_one, "旧版")),
            &author,
        )
        .await,
        StatusCode::CONFLICT,
        "revision_conflict",
    );
    assert_eq!(
        get(&app, &format!("{posts_path}/revisions"), &author)
            .await
            .json["meta"]["pagination"]["total"],
        2
    );
    assert_eq!(
        get(
            &app,
            &format!("{posts_path}/revisions/{revision_one}"),
            &author
        )
        .await
        .json["data"]["title"],
        "用 Axum 构建服务"
    );

    // 审核：作者提交，不能自行决定；编辑拒绝后不能再次决定。
    let review = api(
        &app,
        Method::POST,
        &format!("{posts_path}/reviews"),
        Some(json!({"revision_id": revision_two})),
        &author,
    )
    .await;
    assert_eq!(review.status, StatusCode::CREATED, "{}", review.json);
    let review_path = format!(
        "{posts_path}/reviews/{}",
        review.json["data"]["id"].as_str().unwrap()
    );
    assert_error(
        &api(
            &app,
            Method::POST,
            &format!("{posts_path}/reviews"),
            Some(json!({"revision_id": revision_two})),
            &author,
        )
        .await,
        StatusCode::CONFLICT,
        "review_already_submitted",
    );
    assert_field(
        &api(
            &app,
            Method::POST,
            &format!("{posts_path}/reviews"),
            Some(json!({"revision_id": ids::new_id()})),
            &author,
        )
        .await,
        "revision_id",
        "invalid_value",
    );
    assert_error(
        &api(
            &app,
            Method::PATCH,
            &review_path,
            Some(json!({"decision": "approved"})),
            &author,
        )
        .await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );
    let rejected = api(
        &app,
        Method::PATCH,
        &review_path,
        Some(json!({"decision": "rejected", "comment": " 补充示例 "})),
        &editor,
    )
    .await;
    assert_eq!(rejected.json["data"]["status"], "rejected");
    assert_eq!(rejected.json["data"]["comment"], "补充示例");
    assert_error(
        &api(
            &app,
            Method::PATCH,
            &review_path,
            Some(json!({"decision": "approved"})),
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "review_not_pending",
    );
    assert_error(
        &api(
            &app,
            Method::PUT,
            &format!("{posts_path}/publication"),
            Some(json!({"revision_id": revision_two, "expected_publication_version": 0})),
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "revision_not_approved",
    );

    // 新修订送审并批准后发布；版本号不符时冲突，作者不能发布。
    let saved = api(
        &app,
        Method::POST,
        &format!("{posts_path}/revisions"),
        Some(revision_body(&revision_two, "定稿")),
        &author,
    )
    .await;
    let revision_three = saved.json["data"]["id"].as_str().unwrap().to_owned();
    let review = api(
        &app,
        Method::POST,
        &format!("{posts_path}/reviews"),
        Some(json!({"revision_id": revision_three})),
        &author,
    )
    .await;
    let review_path = format!(
        "{posts_path}/reviews/{}",
        review.json["data"]["id"].as_str().unwrap()
    );
    assert_eq!(
        api(
            &app,
            Method::PATCH,
            &review_path,
            Some(json!({"decision": "approved"})),
            &editor
        )
        .await
        .json["data"]["status"],
        "approved"
    );
    let publication = format!("{posts_path}/publication");
    let publish = |version: i64| json!({"revision_id": revision_three, "expected_publication_version": version});
    assert_error(
        &api(&app, Method::PUT, &publication, Some(publish(0)), &author).await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );
    assert_error(
        &api(&app, Method::PUT, &publication, Some(publish(5)), &editor).await,
        StatusCode::CONFLICT,
        "publication_conflict",
    );
    assert_error(
        &get(&app, "/api/v1/public/site", &author).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "site_not_ready",
    );
    let accepted = api(&app, Method::PUT, &publication, Some(publish(0)), &editor).await;
    assert_eq!(accepted.status, StatusCode::ACCEPTED, "{}", accepted.json);
    assert_eq!(accepted.json["data"]["desired_generation"], 1);
    assert_eq!(accepted.json["data"]["publication_version"], 1);
    let first_job = accepted.json["data"]["publish_job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_error(
        &api(
            &app,
            Method::PATCH,
            &posts_path,
            Some(json!({"slug": "renamed"})),
            &author,
        )
        .await,
        StatusCode::CONFLICT,
        "slug_locked",
    );

    // Worker 构建并上线 generation 1。
    assert_eq!(
        worker.run_once().await.unwrap(),
        Some(PublishOutcome::Activated { generation: 1 })
    );
    assert_eq!(worker.run_once().await.unwrap(), None);
    let detail = get(&app, &posts_path, &author).await;
    assert_eq!(
        detail.json["data"]["publication"]["live_revision_id"],
        revision_three
    );
    assert_eq!(detail.json["data"]["publication"]["active_generation"], 1);
    let job = get(
        &app,
        &format!("/api/v1/admin/publishing/jobs/{first_job}"),
        &editor,
    )
    .await;
    assert_eq!(job.json["data"]["status"], "active");

    // Public API 读取线上快照。
    let site = get(&app, "/api/v1/public/site", &other).await;
    assert_eq!(site.generation(), Some(1));
    assert_eq!(
        site.json["data"]["featured_posts"][0]["slug"],
        "building-with-axum"
    );
    let listed = get(&app, "/api/v1/public/posts?category=backend", &other).await;
    assert_eq!(listed.json["meta"]["pagination"]["total"], 1);
    assert_eq!(listed.json["data"][0]["categories"][0]["name"], "后端");
    assert_eq!(
        get(&app, "/api/v1/public/posts?tag=missing", &other)
            .await
            .json["meta"]["pagination"]["total"],
        0
    );
    let public_post = get(&app, "/api/v1/public/posts/building-with-axum", &other).await;
    assert_eq!(public_post.json["data"]["title"], "定稿");
    assert!(
        public_post.json["data"]["html"]
            .as_str()
            .unwrap()
            .contains("/api/v1/public/media/")
    );
    assert_eq!(
        get(&app, "/api/v1/public/categories", &other).await.json["data"][0]["post_count"],
        1
    );
    assert_eq!(
        get(&app, "/api/v1/public/tags", &other).await.json["data"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "未使用的标签不公开"
    );
    assert_error(
        &get(&app, "/api/v1/public/profile", &other).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    assert!(
        get(&app, "/api/v1/public/ai-trending", &other).await.json["data"]["data_date"].is_null()
    );
    let media = get(&app, &format!("/api/v1/public/media/{cover}"), &other).await;
    assert_eq!((media.status, media.bytes.clone()), (StatusCode::OK, png()));
    assert_error(
        &get(
            &app,
            &format!("/api/v1/public/media/{others_media}"),
            &other,
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    let stale = call(
        &app,
        Method::GET,
        "/api/v1/public/posts",
        None,
        None,
        &[("x-expected-generation", "2")],
    )
    .await;
    assert_error(&stale, StatusCode::CONFLICT, "site_generation_changed");
    assert_eq!(stale.generation(), Some(1));

    // 静态站点：目录映射 index.html，缺少结尾斜杠重定向，未知路径返回该版本的 404 页面。
    let home = call(&site_app, Method::GET, "/", None, None, &[]).await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(
        home.text()
            .contains(r#"name="silva:generation" content="1""#),
        "{}",
        home.text()
    );
    assert_eq!(home.headers[header::CACHE_CONTROL], "no-store");
    let redirect = call(
        &site_app,
        Method::GET,
        "/blog/building-with-axum",
        None,
        None,
        &[],
    )
    .await;
    assert_eq!(redirect.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(
        redirect.headers[header::LOCATION],
        "/blog/building-with-axum/"
    );
    let article = call(
        &site_app,
        Method::GET,
        "/blog/building-with-axum/",
        None,
        None,
        &[],
    )
    .await;
    assert_eq!(article.status, StatusCode::OK);
    assert!(article.text().contains("/api/v1/public/media/"));
    let missing = call(&site_app, Method::GET, "/blog/nope/", None, None, &[]).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(
        missing.headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let traversal = call(&site_app, Method::GET, "/../snapshot.json", None, None, &[]).await;
    assert_eq!(traversal.status, StatusCode::NOT_FOUND);
    let head = call(&site_app, Method::HEAD, "/", None, None, &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.bytes.is_empty());
    if !real_astro {
        let asset = call(&site_app, Method::GET, "/_astro/site.css", None, None, &[]).await;
        assert_eq!(
            asset.headers[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
    }
    let disabled = call(&app, Method::GET, "/", None, None, &[]).await;
    assert_eq!(
        disabled.status,
        StatusCode::NOT_FOUND,
        "未开启 SILVA_SITE_SERVE 时不提供站点"
    );

    // 分类改名：已出现在线上文章中，登记新的站点目标；有修订引用时不能删除，未使用的标签可以删除。
    let renamed = api(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/categories/{category_id}"),
        Some(json!({"name": "服务端"})),
        &editor,
    )
    .await;
    assert_eq!(renamed.json["data"]["item"]["name"], "服务端");
    assert_eq!(renamed.json["data"]["publication"]["desired_generation"], 2);
    let same = api(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/categories/{category_id}"),
        Some(json!({"name": "服务端"})),
        &editor,
    )
    .await;
    assert!(
        same.json["data"]["publication"].is_null(),
        "未变化时不登记站点目标"
    );
    assert_error(
        &api(
            &app,
            Method::DELETE,
            &format!("/api/v1/admin/categories/{category_id}"),
            None,
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "taxonomy_in_use",
    );
    assert_eq!(
        api(
            &app,
            Method::DELETE,
            &format!("/api/v1/admin/tags/{spare_tag_id}"),
            None,
            &editor
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // 构建失败自动重试，用尽后标记为 failed，线上仍为 generation 1。
    builder.fail.store(true, Ordering::SeqCst);
    for attempt in 1..=3 {
        assert!(matches!(
            worker.run_once().await.unwrap(),
            Some(PublishOutcome::Failed { generation: 2, .. })
        ));
        if attempt < 3 {
            // 失败后按退避时间才能再次领取。
            assert_eq!(worker.run_once().await.unwrap(), None);
            sqlx::query("UPDATE publish_jobs SET retry_after = now() WHERE status = 'queued'")
                .execute(pool)
                .await
                .unwrap();
        }
    }
    assert_eq!(worker.run_once().await.unwrap(), None);
    let failed = get(&app, "/api/v1/admin/publishing/jobs?status=failed", &editor).await;
    let failed_job = failed.json["data"][0]["id"].as_str().unwrap().to_owned();
    assert!(
        failed.json["data"][0]["error_message"]
            .as_str()
            .unwrap()
            .contains("simulated build failure")
    );
    assert_eq!(
        get(&app, "/api/v1/public/site", &other).await.generation(),
        Some(1)
    );
    assert_error(
        &api(
            &app,
            Method::POST,
            &format!("/api/v1/admin/publishing/jobs/{failed_job}/attempts"),
            None,
            &editor,
        )
        .await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );
    let retried = api(
        &app,
        Method::POST,
        &format!("/api/v1/admin/publishing/jobs/{failed_job}/attempts"),
        None,
        &admin,
    )
    .await;
    assert_eq!(retried.status, StatusCode::ACCEPTED, "{}", retried.json);
    assert_eq!(retried.json["data"]["status"], "queued");
    builder.fail.store(false, Ordering::SeqCst);
    assert_eq!(
        worker.run_once().await.unwrap(),
        Some(PublishOutcome::Activated { generation: 2 })
    );
    assert_eq!(
        get(&app, "/api/v1/public/categories", &other).await.json["data"][0]["name"],
        "服务端"
    );
    assert_error(
        &api(
            &app,
            Method::POST,
            &format!("/api/v1/admin/publishing/jobs/{first_job}/attempts"),
            None,
            &admin,
        )
        .await,
        StatusCode::CONFLICT,
        "publish_job_not_retryable",
    );
    assert_eq!(
        get(
            &app,
            &format!("/api/v1/admin/publishing/jobs/{first_job}"),
            &editor
        )
        .await
        .json["data"]["status"],
        "retired"
    );

    // 取消发布：排队中的旧目标被新目标取代；上线后文章在公开接口与站点上都返回 404。
    let requested = api(
        &app,
        Method::POST,
        "/api/v1/admin/publishing/jobs",
        None,
        &admin,
    )
    .await;
    assert_eq!(requested.status, StatusCode::ACCEPTED);
    let manual_job = requested.json["data"]["publish_job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        requested.headers[header::LOCATION],
        format!("/api/v1/admin/publishing/jobs/{manual_job}").as_str()
    );
    assert_field(
        &api(&app, Method::DELETE, &publication, None, &editor).await,
        "expected_publication_version",
        "required",
    );
    let unpublished = api(
        &app,
        Method::DELETE,
        &format!("{publication}?expected_publication_version=1"),
        None,
        &editor,
    )
    .await;
    assert_eq!(
        unpublished.status,
        StatusCode::ACCEPTED,
        "{}",
        unpublished.json
    );
    assert_eq!(unpublished.json["data"]["desired_generation"], 4);
    assert_error(
        &api(
            &app,
            Method::DELETE,
            &format!("{publication}?expected_publication_version=2"),
            None,
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "already_unpublished",
    );
    assert_eq!(
        get(
            &app,
            &format!("/api/v1/admin/publishing/jobs/{manual_job}"),
            &editor
        )
        .await
        .json["data"]["status"],
        "superseded"
    );
    assert_eq!(
        worker.run_once().await.unwrap(),
        Some(PublishOutcome::Activated { generation: 4 })
    );
    assert_error(
        &get(&app, "/api/v1/public/posts/building-with-axum", &other).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    assert_eq!(
        get(&app, "/api/v1/public/posts", &other).await.json["meta"]["pagination"]["total"],
        0
    );
    assert_eq!(
        call(
            &site_app,
            Method::GET,
            "/blog/building-with-axum/",
            None,
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let detail = get(&app, &posts_path, &author).await;
    assert!(detail.json["data"]["publication"]["live_revision_id"].is_null());
    assert!(
        detail.json["data"]["slug_locked"].as_bool().unwrap(),
        "取消发布不解锁 slug"
    );

    // 任务可见性：作者只看到自己触发的任务。
    assert_eq!(
        get(&app, "/api/v1/admin/publishing/jobs", &author)
            .await
            .json["meta"]["pagination"]["total"],
        0
    );
    assert_eq!(
        get(&app, "/api/v1/admin/publishing/jobs", &editor)
            .await
            .json["meta"]["pagination"]["total"],
        4
    );
    assert_error(
        &get(
            &app,
            &format!("/api/v1/admin/publishing/jobs/{first_job}"),
            &author,
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
    );

    // 校验后被并发删除的分类：写入时外键拒绝，映射为字段错误而不是 500。
    let repository = silva_infrastructure::content::PgContentRepository::new(pool.clone());
    let current: uuid::Uuid = detail.json["data"]["current_revision_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let author_id: uuid::Uuid = detail.json["data"]["author"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let meta = RequestMeta::default();
    let raced = repository
        .create_revision(
            current_post_id(&posts_path),
            current,
            NewRevision {
                id: ids::new_id(),
                content: RevisionContent {
                    title: "竞态".into(),
                    excerpt: None,
                    markdown_body: "x".into(),
                    category_ids: vec![ids::new_id()],
                    tag_ids: vec![],
                    cover_media_id: None,
                    is_featured: false,
                    seo_title: None,
                    seo_description: None,
                },
                rendered: RenderedMarkdown {
                    html: "<p>x</p>".into(),
                    headings: vec![],
                },
                body_media_ids: vec![],
                created_by: author_id,
            },
            &ContentActor {
                user_id: author_id,
                meta: &meta,
            },
        )
        .await;
    assert!(
        matches!(&raced, Err(AppError::Validation(errors)) if errors[0].field == "category_ids"),
        "{:?}",
        raced.err()
    );

    // 审计覆盖内容与发布操作。
    let actions: Vec<String> = sqlx::query_scalar("SELECT DISTINCT action FROM audit_logs WHERE resource_type IN ('post','category','tag','publish_job') ORDER BY action")
        .fetch_all(pool)
        .await
        .unwrap();
    for expected in [
        "category.create",
        "category.update",
        "post.create",
        "post.publish",
        "post.review_decide",
        "post.review_submit",
        "post.revision_create",
        "post.unpublish",
        "publishing.request",
        "publishing.retry",
        "tag.create",
        "tag.delete",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "缺少审计 {expected}: {actions:?}"
        );
    }
}
