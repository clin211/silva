//! 内容同步上线的验收测试：真实 PostgreSQL、内存对象存储。
//!
//! 使用真实公开接口验证事务提交后立即可见的版本。
//! 通过 `cargo test -p silva-api --test content -- --ignored` 显式执行。
#[path = "content/series.rs"]
mod series;
#[path = "support/storage.rs"]
mod storage;
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use silva_application::ports::{
    ContentActor, ContentRepository, NewRevision, RenderedMarkdown, RequestMeta,
};
use silva_domain::content::RevisionContent;
use silva_domain::site::SiteSnapshot;
use silva_shared::{
    config::{Config, JwtSecret},
    error::AppError,
    ids,
};
use sqlx::{Connection, PgConnection, PgPool, postgres::PgConnectOptions};
use std::{io::Cursor, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use storage::MemoryStorage;
use tower::ServiceExt;

const PASSWORD: &str = "acceptance-password-2026";
const JWT_SECRET: &[u8] = b"acceptance-jwt-secret-0123456789abcdef";

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
    json: Value,
}

impl Response {
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

fn config(database_url: String) -> Config {
    Config {
        database_url,
        bind: "127.0.0.1:0".parse().unwrap(),
        production: false,
        jwt_secret: Some(JwtSecret::new(JWT_SECRET)),
        db_max_connections: 5,
        access_token_ttl: Duration::from_secs(900),
        refresh_token_ttl: Duration::from_secs(3600),
        worker_interval: Duration::from_secs(60),
        trust_proxy_ip_header: false,
        api_rate_per_minute: 10_000,
        login_rate_per_minute: 1000,
        login_lockout_threshold: 1000,
        login_lockout_duration: Duration::from_secs(900),
        storage: None,
        media_upload_ttl: Duration::from_secs(900),
    }
}

/// 从文章路径 `/api/v1/admin/posts/{id}` 取出文章 ID。
fn current_post_id(path: &str) -> uuid::Uuid {
    path.rsplit('/').next().unwrap().parse().unwrap()
}

fn png(width: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        18,
        image::Rgb([10, 90, 160]),
    ));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// 上传封面并设置文章测试使用的替代文本。
async fn ready_media(app: &Router, token: &str, width: u32) -> String {
    let bytes = png(width);
    let (status, _, created) =
        storage::post_image(app, token, "cover.png", "image/png", &bytes, bytes.len()).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["data"]["media"]["id"].as_str().unwrap().to_owned();
    let confirmed = api(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/media/{id}"),
        Some(json!({"alt_text": "封面"})),
        token,
    )
    .await;
    assert_eq!(confirmed.json["data"]["status"], "ready");
    id
}

#[tokio::test]
#[ignore = "需要通过 SILVA_TEST_DATABASE_URL 连接 PostgreSQL，且账户具备 CREATEDB 权限"]
async fn content_publishing_and_public_site() {
    acceptance(false).await;
}

#[tokio::test]
#[ignore = "需要 PostgreSQL 验证系列编排、版本冲突及线上封面保护"]
async fn series_editing_and_publication() {
    acceptance(true).await;
}

async fn acceptance(test_series: bool) {
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
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(async {
        if test_series {
            series::before_upgrade(&pool).await;
        }
        silva_infrastructure::persistence::migrate(&pool)
            .await
            .expect("数据库迁移失败");
        silva_infrastructure::persistence::check_schema(&pool)
            .await
            .expect("数据库不变量验证失败");
        run(&pool, database_url, test_series).await;
    })
    .catch_unwind()
    .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database_name}\" WITH (FORCE)"))
        .execute(&mut admin)
        .await
        .unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

async fn gateway_public_reads(pool: &PgPool, mut config: Config, direct: &Router) {
    let internal = "/internal/site-snapshot";
    let public = "/api/v1/public/site-snapshot";
    assert_eq!(
        call(direct, Method::GET, internal, None, None, &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    config.trust_proxy_ip_header = true;
    config.api_rate_per_minute = 1;
    let gateway = silva_api::build_router(config, pool.clone()).await.unwrap();
    let first = [("x-real-ip", "203.0.113.2")];
    let second = [("x-real-ip", "203.0.113.3")];
    let snapshot = call(&gateway, Method::GET, public, None, None, &first).await;
    assert_eq!(snapshot.status, StatusCode::OK);
    assert_eq!(
        call(&gateway, Method::GET, public, None, None, &first)
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        call(&gateway, Method::GET, public, None, None, &second)
            .await
            .status,
        StatusCode::OK
    );
    let etag = snapshot.headers["etag"].to_str().unwrap();
    // 即使这个客户端用尽公开 API 限额，私网 SSR 的条件读取仍然正常。
    for _ in 0..2 {
        let response = call(
            &gateway,
            Method::GET,
            internal,
            None,
            None,
            &[first[0], ("if-none-match", etag)],
        )
        .await;
        assert_eq!(response.status, StatusCode::NOT_MODIFIED);
        assert_eq!(response.generation(), snapshot.generation());
    }
}

async fn run(pool: &PgPool, database_url: String, test_series: bool) {
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
        config(database_url.clone()),
        pool.clone(),
        Some(storage.clone()),
    )
    .await
    .unwrap();
    if !test_series {
        gateway_public_reads(pool, config(database_url), &app).await;
    }
    let admin = login(&app, "admin@example.test").await;
    let author = login(&app, "author@example.test").await;
    let other = login(&app, "other@example.test").await;
    let editor = login(&app, "editor@example.test").await;

    if test_series {
        series::run(pool, &app, &editor, &author).await;
        return;
    }

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

    // 一个事务正在修改名称时，另一个请求更新说明，两个字段都应保留。
    let category_path = format!("/api/v1/admin/categories/{category_id}");
    let mut rename = pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar(
        "UPDATE categories SET name='临时名称' WHERE id=$1 RETURNING pg_backend_pid()",
    )
    .bind(category_id.parse::<uuid::Uuid>().unwrap())
    .fetch_one(&mut *rename)
    .await
    .unwrap();
    let edit_description = api(
        &app,
        Method::PATCH,
        &category_path,
        Some(json!({"description":"并发更新"})),
        &editor,
    );
    let commit_rename = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))"
                ).bind(blocker).fetch_one(pool).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("分类更新应等待行锁");
        rename.commit().await.unwrap();
    };
    let (patched, ()) = tokio::join!(edit_description, commit_rename);
    assert_eq!(patched.json["data"]["item"]["name"], "临时名称");
    assert_eq!(patched.json["data"]["item"]["description"], "并发更新");
    let restored = api(
        &app,
        Method::PATCH,
        &category_path,
        Some(json!({"name":"后端","description":"服务端实践"})),
        &editor,
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK);

    assert_error(
        &get(&app, "/api/v1/public/posts/%FF", &author).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );

    // 创建文章：正文图片引用受控媒体，原始 HTML 被转义。
    let cover = ready_media(&app, &author, 32).await;
    let others_media = ready_media(&app, &other, 32).await;
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
    assert!(post["publication"]["published_revision_id"].is_null());

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
    let mut stale_revision = revision_body(&revision_one, "旧版");
    stale_revision["category_ids"] = json!([ids::new_id()]);

    assert_error(
        &api(
            &app,
            Method::POST,
            &format!("{posts_path}/revisions"),
            Some(stale_revision),
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
        1
    );
    assert_eq!(
        get(
            &app,
            &format!("{posts_path}/revisions/{revision_one}"),
            &author
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, &format!("{posts_path}/reviews"), &author)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // 有发布权限的用户可直接发布当前保存的内容。
    let saved = api(
        &app,
        Method::POST,
        &format!("{posts_path}/revisions"),
        Some(revision_body(&revision_two, "定稿")),
        &author,
    )
    .await;
    let revision_three = saved.json["data"]["id"].as_str().unwrap().to_owned();
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
    let empty = get(&app, "/api/v1/public/site-snapshot", &author).await;
    assert_eq!(empty.generation(), Some(0));
    assert_eq!(empty.json["data"]["posts"], json!([]));
    // 发布请求到达时该草稿已被更新，返回指向修订字段的错误且保留当前状态。
    let stale_revision = api(
        &app,
        Method::PUT,
        &publication,
        Some(json!({"revision_id":revision_one,"expected_publication_version":0})),
        &editor,
    )
    .await;
    assert_field(&stale_revision, "revision_id", "invalid_value");
    let accepted = api(&app, Method::PUT, &publication, Some(publish(0)), &editor).await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.json);
    assert_eq!(
        accepted.json["data"],
        json!({"generation": 1, "publication_version": 1})
    );
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

    // 继续编辑草稿后，线上仍保留发布时的标题、分类和媒体。
    let later = api(&app, Method::POST, &format!("{posts_path}/revisions"),
        Some(json!({"expected_revision_id": revision_three, "title":"未发布的新稿", "markdown_body":"新内容"})), &author).await;
    assert_eq!(later.status, StatusCode::CREATED, "{}", later.json);
    assert_eq!(
        get(&app, &posts_path, &author).await.json["data"]["current_revision"]["title"],
        "未发布的新稿"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM post_revisions WHERE post_id=$1")
            .bind(current_post_id(&posts_path))
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    let published = get(&app, "/api/v1/public/site-snapshot", &other).await;
    assert_eq!(published.status, StatusCode::OK);
    assert_eq!(published.generation(), Some(1));
    let tag = published.headers[header::ETAG].to_str().unwrap();
    let validated = call(
        &app,
        Method::GET,
        "/api/v1/public/site-snapshot",
        None,
        None,
        &[("if-none-match", tag)],
    )
    .await;
    assert_eq!(validated.status, StatusCode::NOT_MODIFIED);
    assert!(validated.bytes.is_empty());
    let snapshot: SiteSnapshot = serde_json::from_value(published.json["data"].clone()).unwrap();
    assert_eq!(snapshot.posts[0].title, "定稿");
    assert_eq!(snapshot.posts[0].revision_id.to_string(), revision_three);
    let detail = get(&app, &posts_path, &author).await;
    assert_eq!(
        detail.json["data"]["publication"]["published_revision_id"],
        revision_three
    );

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
    assert_eq!(
        (media.status, media.bytes.clone()),
        (StatusCode::OK, png(32))
    );
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

    // 分类改名：已出现在线上文章中，推进公开版本；有修订引用时不能删除，未使用的标签可以删除。
    let renamed = api(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/categories/{category_id}"),
        Some(json!({"name": "服务端"})),
        &editor,
    )
    .await;
    assert_eq!(renamed.json["data"]["item"]["name"], "服务端");
    assert_eq!(renamed.json["data"]["publication"]["generation"], 2);
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
        "未变化时不推进公开版本"
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

    assert_eq!(
        get(&app, "/api/v1/public/site", &other).await.generation(),
        Some(2)
    );
    assert_eq!(
        get(&app, "/api/v1/public/categories", &other).await.json["data"][0]["name"],
        "服务端"
    );
    // 撤回立即对公开文章、索引和媒体生效。
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
    assert_eq!(unpublished.status, StatusCode::OK, "{}", unpublished.json);
    assert_eq!(unpublished.json["data"]["generation"], 3);
    let changed = call(
        &app,
        Method::GET,
        "/api/v1/public/site-snapshot",
        None,
        None,
        &[("if-none-match", tag)],
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_ne!(changed.headers[header::ETAG], tag);
    assert!(changed.json["data"]["posts"].as_array().unwrap().is_empty());
    assert_error(
        &get(&app, &format!("/api/v1/public/media/{cover}"), &other).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
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
        get(&app, "/api/v1/public/site-snapshot", &other).await.json["data"]["posts"],
        json!([])
    );
    let detail = get(&app, &posts_path, &author).await;
    assert!(detail.json["data"]["publication"]["published_revision_id"].is_null());
    assert!(
        detail.json["data"]["slug_locked"].as_bool().unwrap(),
        "取消发布不解锁 slug"
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
        "post.revision_create",
        "post.unpublish",
        "tag.create",
        "tag.delete",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "缺少审计 {expected}: {actions:?}"
        );
    }
    profile_publishing(pool, &app, (&admin, &editor, &author)).await;
    // 撤下后再次发布当前草稿，公开内容与本次编辑一致。
    let latest = get(&app, &posts_path, &editor).await.json["data"].clone();
    let republished = api(&app, Method::PUT, &publication,
        Some(json!({"revision_id": latest["current_revision_id"], "expected_publication_version": latest["publication"]["publication_version"]})), &editor).await;
    assert_eq!(republished.status, StatusCode::OK);
    assert_eq!(
        get(&app, "/api/v1/public/posts/building-with-axum", &editor)
            .await
            .json["data"]["title"],
        "未发布的新稿"
    );

    // 对同一篇文章验证权限、版本保护，以及撤下前后的删除边界。
    let rev = latest["current_revision_id"].as_str().unwrap();
    let version = republished.json["data"]["publication_version"]
        .as_i64()
        .unwrap();
    let delete_url = |revision: &str, version: i64| {
        format!(
            "{posts_path}?expected_revision_id={revision}&expected_publication_version={version}"
        )
    };
    assert_field(
        &api(&app, Method::DELETE, &posts_path, None, &editor).await,
        "expected_revision_id",
        "required",
    );
    let target = delete_url(rev, version);
    assert_error(
        &api(&app, Method::DELETE, &target, None, &author).await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );
    assert_error(
        &api(
            &app,
            Method::DELETE,
            &delete_url(&revision_one, version),
            None,
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "revision_conflict",
    );
    assert_error(
        &api(
            &app,
            Method::DELETE,
            &delete_url(rev, version - 1),
            None,
            &editor,
        )
        .await,
        StatusCode::CONFLICT,
        "publication_conflict",
    );
    assert_error(
        &api(&app, Method::DELETE, &target, None, &editor).await,
        StatusCode::CONFLICT,
        "post_in_use",
    );
    let withdrawn = api(
        &app,
        Method::DELETE,
        &format!("{publication}?expected_publication_version={version}"),
        None,
        &editor,
    )
    .await;
    assert_eq!(withdrawn.status, StatusCode::OK);
    let target = delete_url(rev, version + 1);
    assert_eq!(
        api(&app, Method::DELETE, &target, None, &editor)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_error(
        &get(&app, &posts_path, &editor).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    assert_error(
        &api(&app, Method::DELETE, &target, None, &editor).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    let deleted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action='post.delete' AND resource_id=$1",
    )
    .bind(current_post_id(&posts_path))
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(deleted, 1);
}

async fn profile_publishing(
    pool: &PgPool,
    app: &Router,
    (admin, editor, author): (&str, &str, &str),
) {
    let path = "/api/v1/admin/users/profile";
    let public = "/api/v1/public/profile";
    let avatar = ready_media(app, author, 16).await;
    let first = json!({
        "display_name": "Profile One", "avatar_media_id": avatar, "bio_md": "**hello**",
        "experiences": [{"organization": "Silva", "role": "维护者", "description_md": "**经验**",
            "started_on": "2024-02-29", "ended_on": null}],
        "skills": [{"name":"Rust"}, {"name":"Go", "category":"后端"}],
        "links": [{"platform":"mail", "url":"mailto:hello@example.test"}]
    });
    assert_error(
        &get(app, "/api/v1/admin/profile", admin).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    assert_error(
        &get(app, path, admin).await,
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
    );
    assert_error(
        &api(app, Method::PUT, path, Some(first.clone()), author).await,
        StatusCode::FORBIDDEN,
        "forbidden",
    );

    // 并发首次保存只能创建一份资料，且共用同一公开版本。
    let (a, b) = tokio::join!(
        api(app, Method::PUT, path, Some(first.clone()), editor),
        api(app, Method::PUT, path, Some(first.clone()), admin)
    );
    assert_eq!(a.status, StatusCode::OK, "{}", a.json);
    assert_eq!(b.status, StatusCode::OK, "{}", b.json);
    assert_eq!(a.json, b.json);
    let generation = a.json["data"]["generation"].as_i64().unwrap();
    let mut invalid = first.clone();
    invalid["experiences"][0]["ended_on"] = json!("2020-01-01");
    invalid["links"][0]["url"] = json!("relative");
    let response = api(app, Method::PUT, path, Some(invalid), editor).await;
    assert_field(&response, "experiences[0].ended_on", "invalid_value");
    assert_field(&response, "links[0].url", "invalid_format");
    let mut invalid = first.clone();
    let pending = ids::new_id();
    invalid["avatar_media_id"] = json!(pending);
    assert_field(
        &api(app, Method::PUT, path, Some(invalid.clone()), editor).await,
        "avatar_media_id",
        "invalid_value",
    );
    sqlx::query("INSERT INTO media_assets (id, upload_key, original_filename, declared_mime_type, declared_size_bytes, upload_expires_at) VALUES ($1,'pending-avatar','avatar.png','image/png',1,now())")
        .bind(pending).execute(pool).await.unwrap();
    assert_error(
        &api(app, Method::PUT, path, Some(invalid), editor).await,
        StatusCode::CONFLICT,
        "media_not_ready",
    );

    let live = get(app, public, author).await;
    assert_eq!(live.generation(), Some(generation));
    assert_eq!(live.json["data"]["avatar"]["media_id"], avatar);
    assert_eq!(live.json["data"]["skills"][1]["name"], "Go");
    assert!(
        live.json["data"]["bio_html"]
            .as_str()
            .unwrap()
            .contains("<strong>hello</strong>")
    );
    assert!(
        live.json["data"]["experiences"][0]["description_html"]
            .as_str()
            .unwrap()
            .contains("<strong>经验</strong>")
    );
    let usage = get(app, &format!("/api/v1/admin/media/{avatar}"), editor).await;
    assert_eq!(usage.json["meta"]["reference_count"], 1);
    assert_eq!(
        usage.json["meta"]["references"][0]["kind"],
        "profile_avatar"
    );
    let second =
        json!({"display_name": "Profile Two", "experiences": [], "skills": [], "links": []});
    let saved = api(app, Method::PUT, path, Some(second), admin).await;
    let next = saved.json["data"]["generation"].as_i64().unwrap();
    assert_eq!(next, generation + 1);
    let media_path = format!("/api/v1/admin/media/{avatar}");
    assert_error(
        &get(app, &format!("/api/v1/public/media/{avatar}"), author).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    let refreshed = get(app, public, author).await;
    assert_eq!(refreshed.generation(), Some(next));
    assert_eq!(refreshed.json["data"]["display_name"], "Profile Two");
    assert!(refreshed.json["data"]["avatar"].is_null());
    assert_eq!(refreshed.json["data"]["skills"], json!([]));
    assert_eq!(refreshed.json["data"]["experiences"], json!([]));
    assert_eq!(
        api(app, Method::DELETE, &media_path, None, editor)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let saves: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action='profile.update'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(saves, 2);
}
