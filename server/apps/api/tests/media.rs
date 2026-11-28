//! 媒体上传的验收测试：真实 PostgreSQL、真实图片解码，对象存储使用内存实现。
//! 通过 `cargo test -p silva-api --test media -- --ignored` 显式执行。
#[path = "support/storage.rs"]
mod storage;
mod support;

use storage::MemoryStorage;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use silva_application::ports::MediaRepository;
use silva_shared::{
    config::{Config, JwtSecret},
    ids,
};
use sqlx::{Connection, PgConnection, PgPool, postgres::PgConnectOptions};
use std::{io::Cursor, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "acceptance-password-2026";
const JWT_SECRET: &[u8] = b"acceptance-jwt-secret-0123456789abcdef";

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
    json: Value,
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: &str,
) -> Response {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::USER_AGENT, "silva-media-acceptance/1.0")
        .extension(ConnectInfo(
            "192.0.2.10:40000".parse::<SocketAddr>().unwrap(),
        ));
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
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
    support::assert_response(&method, path, status, &headers, &bytes);
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

fn assert_error(response: &Response, status: StatusCode, code: &str) {
    assert_eq!(response.status, status, "{}", response.json);
    assert_eq!(response.json["error"]["code"], code, "{}", response.json);
}

async fn login(app: &Router, email: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/admin/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .extension(ConnectInfo(
                    "192.0.2.10:40000".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(
                    json!({"email": email, "password": PASSWORD}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
    body["data"]["access_token"].as_str().unwrap().to_owned()
}

fn encode(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([30, 120, 200]),
    ));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
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

async fn upload(
    app: &Router,
    token: &str,
    name: &str,
    mime: &str,
    bytes: &[u8],
    length: usize,
) -> Response {
    let (status, headers, json) = storage::post_image(app, token, name, mime, bytes, length).await;
    Response {
        status,
        headers,
        json,
        bytes: Vec::new(),
    }
}

/// 模拟升级前已申请的临时记录，用于验证过期清理兼容性。
async fn pending(pool: &PgPool, name: &str, owner: Uuid) -> Uuid {
    let id = ids::new_id();
    silva_infrastructure::media::PgMediaRepository::new(pool.clone())
        .create(
            &silva_application::ports::NewMedia {
                id,
                upload_key: format!("uploads/{id}"),
                original_filename: name.into(),
                declared_mime_type: "image/png".into(),
                declared_size_bytes: 20,
                upload_expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
                uploaded_by: owner,
            },
            &Default::default(),
        )
        .await
        .unwrap();
    id
}

async fn confirm(app: &Router, token: &str, id: Uuid) -> Response {
    request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/media/{id}"),
        Some(json!({"status": "uploaded"})),
        token,
    )
    .await
}

#[tokio::test]
#[ignore = "需要通过 SILVA_TEST_DATABASE_URL 连接 PostgreSQL，且账户具备 CREATEDB 权限"]
async fn media_upload_lifecycle() {
    let database_url = std::env::var("SILVA_TEST_DATABASE_URL")
        .expect("set SILVA_TEST_DATABASE_URL; real PostgreSQL tests never silently skip");
    let options = PgConnectOptions::from_str(&database_url).unwrap();
    let mut admin = PgConnection::connect_with(&options).await.unwrap();
    let database_name = format!("silva_media_{}", ids::new_id().simple());
    sqlx::query(&format!("CREATE DATABASE \"{database_name}\""))
        .execute(&mut admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(options.database(&database_name))
        .await
        .unwrap();
    // 迁移后先在任务外执行 schema 校验（返回 Result，不会 panic），再在任务中运行断言；
    // 清理逻辑放在任务外，确保断言失败后仍会删除隔离测试库。
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
    result.unwrap().expect("media acceptance assertions failed");
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
    let author = login(&app, "author@example.test").await;
    let other = login(&app, "other@example.test").await;
    let editor = login(&app, "editor@example.test").await;

    let service = silva_infrastructure::media_service(
        pool.clone(),
        Some(storage.clone()),
        &config(database_url.clone()),
    );
    shared_uploads(&app, &author, &other, &service, &storage, pool).await;
    let png = encode(40, 30, image::ImageFormat::Png);
    let created = upload(
        &app,
        &author,
        "",
        "application/octet-stream",
        &png,
        png.len(),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let media = &created.json["data"]["media"];
    let id: Uuid = media["id"].as_str().unwrap().parse().unwrap();
    let owner: Uuid = media["uploaded_by"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        created.headers[header::LOCATION],
        format!("/api/v1/admin/media/{id}")
    );
    let key: String = sqlx::query_scalar("SELECT storage_key FROM media_assets WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        created.json["data"]["url"],
        format!("https://storage.test/{key}?read=1")
    );
    assert_eq!(media["original_filename"], "image");
    assert_eq!(media["status"], "ready");
    assert_eq!(media["mime_type"], "image/png");
    assert_eq!(media["size_bytes"], png.len());
    assert_eq!(media["width"], 40);
    assert_eq!(media["height"], 30);
    assert_eq!(storage.keys(), std::slice::from_ref(&key));
    let edited = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/media/{id}"),
        Some(json!({"alt_text":"  热气球封面 "})),
        &author,
    )
    .await;
    assert_eq!(edited.json["data"]["alt_text"], "热气球封面");
    let reused = upload(&app, &author, "renamed.png", "image/png", &png, png.len()).await;
    assert_eq!(reused.status, StatusCode::OK);
    assert_eq!(reused.json["data"]["media"]["id"], id.to_string());
    assert_eq!(reused.json["data"]["media"]["original_filename"], "image");
    assert_eq!(reused.json["data"]["media"]["alt_text"], "热气球封面");
    // 授权读取内容：原始字节、实际类型、不缓存并带沙箱 CSP。
    let content = request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/media/{id}/content"),
        None,
        &author,
    )
    .await;
    assert_eq!(content.status, StatusCode::OK);
    assert_eq!(content.bytes, png);
    assert_eq!(
        content.headers[header::CONTENT_LENGTH],
        png.len().to_string().as_str()
    );
    assert_eq!(content.headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(content.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        content.headers[header::CONTENT_SECURITY_POLICY],
        "default-src 'none'; sandbox"
    );

    // 归属：其他作者看不到，编辑可以看到全部。
    for (method, path) in [
        (Method::GET, format!("/api/v1/admin/media/{id}")),
        (Method::GET, format!("/api/v1/admin/media/{id}/content")),
        (Method::DELETE, format!("/api/v1/admin/media/{id}")),
    ] {
        assert_error(
            &request(&app, method, &path, None, &other).await,
            StatusCode::NOT_FOUND,
            "not_found",
        );
    }
    let listed = request(&app, Method::GET, "/api/v1/admin/media", None, &other).await;
    assert_eq!(listed.json["meta"]["pagination"]["total"], 0);
    let listed = request(
        &app,
        Method::GET,
        "/api/v1/admin/media?status=ready",
        None,
        &editor,
    )
    .await;
    assert_eq!(listed.json["data"][0]["id"], id.to_string());

    // 按实际字节识别类型：声明 JPEG、实际 WebP 时以 WebP 记录。
    let webp = encode(8, 8, image::ImageFormat::WebP);
    let result = upload(&app, &other, "photo.jpg", "image/jpeg", &webp, webp.len()).await;
    let media = &result.json["data"]["media"];
    let webp_id: Uuid = media["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(media["mime_type"], "image/webp");
    assert_eq!(media["declared_mime_type"], "image/jpeg");
    let corrupt = &png[..24];
    let rejected = upload(
        &app,
        &author,
        "bad.png",
        "image/png",
        corrupt,
        corrupt.len(),
    )
    .await;
    assert_error(&rejected, StatusCode::UNPROCESSABLE_ENTITY, "media_invalid");
    let bad_id: Uuid = sqlx::query_scalar("SELECT id FROM media_assets WHERE status='rejected'")
        .fetch_one(pool)
        .await
        .unwrap();
    let detail = request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/media/{bad_id}"),
        None,
        &author,
    )
    .await;
    assert_eq!(detail.json["data"]["status"], "rejected");
    assert_eq!(detail.json["data"]["rejection_reason"], "undecodable");
    assert_error(
        &request(
            &app,
            Method::GET,
            &format!("/api/v1/admin/media/{bad_id}/content"),
            None,
            &author,
        )
        .await,
        StatusCode::CONFLICT,
        "media_not_ready",
    );
    assert!(!storage.keys().contains(&format!("uploads/{bad_id}")));

    // 过期后确认返回 409 upload_expired，记录保持 pending。
    let late_id = pending(pool, "late.png", owner).await;
    sqlx::query(
        "UPDATE media_assets SET upload_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(late_id)
    .execute(pool)
    .await
    .unwrap();
    assert_error(
        &confirm(&app, &author, late_id).await,
        StatusCode::CONFLICT,
        "upload_expired",
    );

    // 超限在读取请求体前拒绝；实际字节数也必须匹配。
    let oversized = upload(&app, &author, "x.png", "image/png", &[], 10_485_761).await;
    assert_error(
        &oversized,
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
    );
    let invalid = upload(&app, &author, "short.png", "image/png", &png[..7], 6).await;
    assert_error(
        &invalid,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    );
    let invalid = upload(&app, &author, "fake.png", "image/png", &[42; 24], 24).await;
    assert_error(&invalid, StatusCode::UNPROCESSABLE_ENTITY, "media_invalid");
    // 被个人资料或文章修订引用时不能删除；解除引用后删除记录和正式对象。
    sqlx::query(
        "INSERT INTO profiles (id, display_name, avatar_media_id) VALUES ($1, 'Owner', $2)",
    )
    .bind(ids::new_id())
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    let path = format!("/api/v1/admin/media/{id}");
    let conflict = request(&app, Method::DELETE, &path, None, &author).await;
    assert_eq!(
        conflict.json["error"]["media_usage"],
        json!({"reference_count":1,"references":[]})
    );
    let detail = request(&app, Method::GET, &path, None, &editor).await;
    assert_eq!(
        detail.json["meta"]["references"][0]["kind"],
        "profile_avatar"
    );
    assert_error(
        &request(&app, Method::DELETE, &path, None, &author).await,
        StatusCode::CONFLICT,
        "media_in_use",
    );
    sqlx::query("DELETE FROM profiles")
        .execute(pool)
        .await
        .unwrap();
    let (post_id, revision_id) = (ids::new_id(), ids::new_id());
    // 文章与首条修订互相引用，用单条语句在同一事务内写入。
    sqlx::query(
        "WITH post AS (INSERT INTO posts (id, slug, current_revision_id) VALUES ($1, 'media-probe', $2)) \
         INSERT INTO post_revisions (id, post_id, revision_no, title, markdown_body, cover_media_id) \
         VALUES ($2, $1, 1, 'Probe', '# Probe', $3)",
    )
    .bind(post_id)
    .bind(revision_id)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO post_revision_media VALUES ($1,$2)")
        .bind(revision_id)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    let detail = request(&app, Method::GET, &path, None, &editor).await;
    assert_eq!(detail.json["meta"]["reference_count"], 2);
    assert_eq!(detail.json["meta"]["references"][0]["kind"], "post_body");
    assert_eq!(detail.json["meta"]["references"][1]["kind"], "post_cover");
    assert_eq!(
        detail.json["meta"]["references"][1]["revision_id"],
        revision_id.to_string()
    );
    assert_error(
        &request(&app, Method::DELETE, &path, None, &editor).await,
        StatusCode::CONFLICT,
        "media_in_use",
    );
    sqlx::query("DELETE FROM posts")
        .execute(pool)
        .await
        .unwrap();
    let deleted = request(&app, Method::DELETE, &path, None, &author).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    service.purge_stale(chrono::Utc::now()).await.unwrap();
    assert!(!storage.keys().contains(&key));
    assert_error(
        &request(&app, Method::GET, &path, None, &author).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );

    // 审计：媒体操作记录为 resource_type=media。
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE resource_type='media' AND resource_id=$1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        actions,
        [
            "media.create",
            "media.ready",
            "media.update",
            "media.delete"
        ]
    );
    let rejected: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action='media.rejected' AND resource_id=$1 AND metadata->>'reason'='undecodable'",
    )
    .bind(bad_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(rejected, 1, "重复确认不重复审计");

    // Worker 清理：过期 pending 在宽限期后删除记录和临时对象，ready 保留。
    let media = silva_infrastructure::media_service(
        pool.clone(),
        Some(storage.clone()),
        &config(database_url.clone()),
    );
    let purged = media
        .purge_stale(chrono::Utc::now() + chrono::Duration::hours(2))
        .await
        .unwrap();
    assert_eq!((purged.purged, purged.failed), (1, 0));
    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM media_assets ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap();
    assert!(!remaining.contains(&late_id));
    assert!(remaining.contains(&webp_id) && remaining.contains(&bad_id));
    assert!(!storage.keys().contains(&format!("uploads/{late_id}")));

    // 已拒绝记录按拒绝时间计算保留期：修改替代文本刷新 updated_at，但 7 天后仍被清理。
    let edited = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/media/{bad_id}"),
        Some(json!({"alt_text": "动图"})),
        &author,
    )
    .await;
    assert_eq!(edited.status, StatusCode::OK);
    let (rejected_at, updated_at): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT rejected_at, updated_at FROM media_assets WHERE id = $1")
            .bind(bad_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(updated_at > rejected_at);
    let purged = media
        .purge_stale(chrono::Utc::now() + chrono::Duration::days(8))
        .await
        .unwrap();
    assert_eq!((purged.purged, purged.failed), (1, 0));
    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM media_assets")
        .fetch_all(pool)
        .await
        .unwrap();
    assert_eq!(remaining, [webp_id], "ready 媒体不受清理影响");

    let legacy = pending(pool, "old.webp", owner).await;
    let old_key = format!("media/{legacy}");
    storage
        .objects
        .lock()
        .unwrap()
        .insert(old_key.clone(), webp.clone());
    sqlx::query("UPDATE media_assets SET status='ready',storage_key=$2,mime_type='image/webp',size_bytes=$3,width=8,height=8 WHERE id=$1")
        .bind(legacy).bind(&old_key).bind(webp.len() as i64).execute(pool).await.unwrap();
    assert_eq!(
        service
            .purge_stale(chrono::Utc::now())
            .await
            .unwrap()
            .failed,
        0
    );
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT storage_key FROM media_assets WHERE id=ANY($1)")
            .bind(vec![legacy, webp_id])
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(keys.len(), 1);
    assert!(storage.keys().contains(&old_key));
    let bytes = request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/media/{legacy}/content"),
        None,
        &author,
    )
    .await;
    assert_eq!(bytes.bytes, webp);
    sqlx::query("UPDATE media_object_deletions SET retry_at=now() WHERE storage_key=$1")
        .bind(&old_key)
        .execute(pool)
        .await
        .unwrap();
    service.purge_stale(chrono::Utc::now()).await.unwrap();
    assert!(!storage.keys().contains(&old_key));
    // 升级前签发的直传也共享现有文件，确认及重试始终返回已签发的媒体 ID。
    let old_upload = pending(pool, "pending.webp", owner).await;
    let upload_key = format!("uploads/{old_upload}");
    storage
        .objects
        .lock()
        .unwrap()
        .insert(upload_key.clone(), webp.clone());
    for _ in 0..2 {
        let confirmed = confirm(&app, &author, old_upload).await;
        assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.json);
        assert_eq!(confirmed.json["data"]["id"], old_upload.to_string());
    }
    let actual: String = sqlx::query_scalar("SELECT storage_key FROM media_assets WHERE id=$1")
        .bind(old_upload)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(actual, keys[0]);
    assert!(!storage.keys().contains(&upload_key));

    // 单条清理失败不中断整批，失败记录排到后面；仍被引用的过期记录不清理。
    let mut expired = Vec::new();
    for name in ["a.png", "b.png", "c.png"] {
        let id = pending(pool, name, owner).await;
        expired.push(id);
    }
    sqlx::query(
        "UPDATE media_assets SET upload_expires_at = now() - interval '2 hours' WHERE id = ANY($1)",
    )
    .bind(&expired)
    .execute(pool)
    .await
    .unwrap();
    let (failing, referenced, normal) = (expired[0], expired[1], expired[2]);
    storage
        .failing_deletes
        .lock()
        .unwrap()
        .push(format!("uploads/{failing}"));
    sqlx::query(
        "INSERT INTO profiles (id, display_name, avatar_media_id) VALUES ($1, 'Owner', $2)",
    )
    .bind(ids::new_id())
    .bind(referenced)
    .execute(pool)
    .await
    .unwrap();
    let report = media.purge_stale(chrono::Utc::now()).await.unwrap();
    assert_eq!((report.purged, report.failed), (1, 1));
    let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM media_assets")
        .fetch_all(pool)
        .await
        .unwrap();
    assert!(remaining.contains(&failing) && remaining.contains(&referenced));
    assert!(!remaining.contains(&normal));
    let attempted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT purge_attempted_at FROM media_assets WHERE id = $1")
            .bind(failing)
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(attempted.is_some(), "失败的记录被标记");
    let later = pending(pool, "d.png", owner).await;
    sqlx::query(
        "UPDATE media_assets SET upload_expires_at = now() - interval '2 hours' WHERE id = $1",
    )
    .bind(later)
    .execute(pool)
    .await
    .unwrap();
    let order: Vec<Uuid> = silva_infrastructure::media::PgMediaRepository::new(pool.clone())
        .stale(chrono::Utc::now(), chrono::Utc::now(), 10)
        .await
        .unwrap()
        .into_iter()
        .map(|media| media.id)
        .collect();
    assert_eq!(order, [later, failing], "被引用的不返回，失败过的排在后面");
    sqlx::query("DELETE FROM profiles")
        .execute(pool)
        .await
        .unwrap();

    // 未配置存储：申请上传返回 503，列表仍可读取。
    let unconfigured =
        silva_api::build_router_with_storage(config(database_url), pool.clone(), None)
            .await
            .unwrap();
    assert_error(
        &upload(
            &unconfigured,
            &author,
            "a.png",
            "image/png",
            &png,
            png.len(),
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
    );
    assert_eq!(
        request(
            &unconfigured,
            Method::GET,
            "/api/v1/admin/media",
            None,
            &author
        )
        .await
        .status,
        StatusCode::OK
    );
}

/// 三个请求竞争同一份新文件；模拟回收失败后重新上传，旧任务不能误删新对象。
async fn shared_uploads(
    app: &Router,
    author: &str,
    other: &str,
    service: &silva_application::media::MediaService,
    storage: &MemoryStorage,
    pool: &PgPool,
) {
    let bytes = encode(11, 7, image::ImageFormat::Png);
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (resume, paused) = tokio::sync::oneshot::channel();
    *storage.pause_next_put.lock().unwrap() = Some(storage::WritePause {
        entered,
        resume: paused,
    });
    let check_lock = async {
        let key = tokio::time::timeout(Duration::from_secs(10), waiting)
            .await
            .unwrap()
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        // 对象请求仍挂起；新事务必须能立刻取得同一行的排他锁。
        sqlx::query("SELECT sha256 FROM media_blobs WHERE storage_key=$1 FOR UPDATE NOWAIT")
            .bind(&key)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE media_blobs SET created_at=now()-interval '2 hours' WHERE storage_key=$1",
        )
        .bind(&key)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        service.purge_stale(chrono::Utc::now()).await.unwrap();
        let reserved: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT FROM media_blobs b
            JOIN media_blob_writes w USING(sha256) WHERE b.storage_key=$1)",
        )
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(reserved, "回收任务必须保留正在写入的对象键");
        let (second, third) = tokio::join!(
            upload(app, author, "b.png", "image/png", &bytes, bytes.len()),
            upload(app, other, "c.png", "image/png", &bytes, bytes.len())
        );
        resume.send(()).unwrap();
        (second, third)
    };
    let (first, (second, third)) = tokio::join!(
        upload(app, author, "a.png", "image/png", &bytes, bytes.len()),
        check_lock
    );
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CREATED]);
    assert_eq!(third.status, StatusCode::CREATED);
    let mine = &first.json["data"]["media"]["id"];
    assert_eq!(mine, &second.json["data"]["media"]["id"]);
    let theirs = &third.json["data"]["media"]["id"];
    assert_ne!(mine, theirs);
    let paths = [mine, theirs].map(|id| format!("/api/v1/admin/media/{}", id.as_str().unwrap()));
    let keys = storage.keys();
    assert_eq!(keys.len(), 1);
    assert_eq!(
        request(app, Method::DELETE, &paths[0], None, author)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    service.purge_stale(chrono::Utc::now()).await.unwrap();
    assert_eq!(
        request(
            app,
            Method::GET,
            &format!("{}/content", paths[1]),
            None,
            other
        )
        .await
        .bytes,
        bytes
    );
    assert_eq!(
        request(app, Method::DELETE, &paths[1], None, other)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    storage
        .failing_deletes
        .lock()
        .unwrap()
        .push(keys[0].clone());
    assert_eq!(
        service
            .purge_stale(chrono::Utc::now())
            .await
            .unwrap()
            .failed,
        1
    );
    let again = upload(app, author, "new.png", "image/png", &bytes, bytes.len()).await;
    assert_eq!(again.status, StatusCode::CREATED);
    storage.failing_deletes.lock().unwrap().clear();
    sqlx::query("UPDATE media_object_deletions SET retry_at=now()")
        .execute(pool)
        .await
        .unwrap();
    service.purge_stale(chrono::Utc::now()).await.unwrap();
    assert_eq!(storage.keys().len(), 1);
    assert_ne!(storage.keys(), keys);
    let path = again
        .headers
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(
        request(app, Method::DELETE, path, None, author)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    service.purge_stale(chrono::Utc::now()).await.unwrap();
    assert!(storage.keys().is_empty());
}
