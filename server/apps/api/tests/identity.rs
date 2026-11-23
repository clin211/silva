//! 使用真实 PostgreSQL 的验收测试，通过 `cargo test -p silva-api --test identity -- --ignored` 显式执行。
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use silva_shared::{config::Config, ids};
use sqlx::{Connection, PgConnection, PgPool, postgres::PgConnectOptions};
use std::{net::SocketAddr, str::FromStr, time::Duration};
use tower::ServiceExt;
use uuid::{Uuid, Variant};

const ORIGIN: &str = "https://admin.example.test";
const PASSWORD: &str = "acceptance-password-2026";
const TEST_USER_AGENT: &str = "silva-acceptance/1.0";
const TEST_PEER: &str = "[2001:db8::7]:40000";

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
    origin: Option<&str>,
) -> Response {
    let body = body.map(|value| value.to_string());
    raw_request(
        app,
        method,
        path,
        body.as_deref(),
        Some("application/json"),
        cookie,
        csrf,
        origin,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn raw_request(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    cookie: Option<&str>,
    csrf: Option<&str>,
    origin: Option<&str>,
) -> Response {
    let mut builder = Request::builder().method(method.clone()).uri(path);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    if let Some(value) = csrf {
        builder = builder.header("x-csrf-token", value);
    }
    if let Some(value) = origin {
        builder = builder.header(header::ORIGIN, value);
    }
    // 模拟真实连接的 peer 地址和浏览器 UA，验证审计来源信息。
    builder = builder
        .header(header::USER_AGENT, TEST_USER_AGENT)
        .extension(ConnectInfo(TEST_PEER.parse::<SocketAddr>().unwrap()));
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(body.unwrap_or("").to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    support::assert_response(&method, path, status, &headers, &bytes);
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            panic!(
                "non-JSON response ({status}): {}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    Response {
        status,
        headers,
        json,
    }
}

/// 断言 422 校验错误，并且 `details` 中包含指定字段与字段错误码。
fn assert_field_error(response: &Response, field: &str, code: &str) {
    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    );
    let details = response.json["error"]["details"].as_array().unwrap();
    assert!(
        details
            .iter()
            .any(|detail| detail["field"] == field && detail["code"] == code),
        "缺少字段错误 {field}/{code}：{}",
        response.json
    );
}

fn assert_error(response: &Response, status: StatusCode, code: &str) {
    assert_eq!(response.status, status, "{}", response.json);
    assert_eq!(response.json["error"]["code"], code, "{}", response.json);
    assert!(response.json["error"]["message"].is_string());
    assert!(response.json["error"]["details"].is_array());
    let request_id = response.json["error"]["request_id"].as_str().unwrap();
    let id = Uuid::parse_str(request_id).unwrap();
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(id.get_variant(), Variant::RFC4122);
    assert_eq!(response.headers["x-request-id"], request_id);
}

async fn login(app: &Router, email: &str) -> (String, String, Value) {
    let response = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email": email, "password": PASSWORD})),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.json);
    assert_eq!(response.headers[header::CACHE_CONTROL], "no-store");
    let cookie_header = response.headers[header::SET_COOKIE].to_str().unwrap();
    let attributes = cookie_header.to_ascii_lowercase();
    for required in [
        "__host-silva_session=",
        "httponly",
        "secure",
        "samesite=lax",
        "path=/",
    ] {
        assert!(attributes.contains(required), "{cookie_header}");
    }
    assert!(!attributes.contains("domain="), "{cookie_header}");
    let cookie = cookie_header.split(';').next().unwrap().to_owned();
    let session = response.json["data"].clone();
    let csrf = session["csrf_token"].as_str().unwrap().to_owned();
    assert!(!csrf.is_empty());
    assert!(session["permissions"].is_array());
    (cookie, csrf, session)
}

fn config(database_url: String) -> Config {
    Config {
        database_url,
        bind: "127.0.0.1:0".parse().unwrap(),
        admin_origin: ORIGIN.into(),
        db_max_connections: 5,
        session_ttl: Duration::from_secs(3600),
        worker_interval: Duration::from_secs(60),
        api_rate_per_minute: 10_000,
        login_rate_per_minute: 1000,
        login_lockout_threshold: 1000,
        login_lockout_duration: Duration::from_secs(900),
    }
}

#[tokio::test]
#[ignore = "需要通过 SILVA_TEST_DATABASE_URL 连接 PostgreSQL，且账户具备 CREATEDB 权限"]
async fn identity_security_and_account_lifecycle() {
    let database_url = std::env::var("SILVA_TEST_DATABASE_URL")
        .expect("set SILVA_TEST_DATABASE_URL; real PostgreSQL tests never silently skip");
    let options = PgConnectOptions::from_str(&database_url).unwrap();
    let mut admin = PgConnection::connect_with(&options).await.unwrap();
    let database_name = format!("silva_identity_{}", ids::new_id().simple());
    sqlx::query(&format!("CREATE DATABASE \"{database_name}\""))
        .execute(&mut admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(options.database(&database_name))
        .await
        .unwrap();
    let test_pool = pool.clone();
    // 将清理逻辑放在任务外，确保断言触发 panic 后仍会删除隔离测试库。
    let result = tokio::spawn(async move {
        silva_infrastructure::persistence::migrate(&test_pool)
            .await
            .unwrap();
        let service =
            silva_infrastructure::identity_service(test_pool.clone(), Duration::from_secs(3600))
                .await
                .unwrap();
        service
            .seed_admin(
                "admin@example.test".into(),
                "Acceptance Admin".into(),
                PASSWORD.into(),
            )
            .await
            .unwrap();
        let app = silva_api::build_router(config(database_url.clone()), test_pool.clone())
            .await
            .unwrap();
        run_acceptance(&app, &test_pool).await;
        run_rate_limits(&test_pool, &database_url).await;
        run_login_lockout(&test_pool, &database_url).await;
    })
    .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database_name}\" WITH (FORCE)"))
        .execute(&mut admin)
        .await
        .unwrap();
    result.expect("identity acceptance assertions failed");
}

async fn run_acceptance(app: &Router, pool: &PgPool) {
    for path in ["/health/live", "/health/ready"] {
        let response = request(app, Method::GET, path, None, None, None, None).await;
        assert_eq!(response.status, StatusCode::OK, "{path}: {}", response.json);
        assert_eq!(response.json["status"], "ok");
    }
    let openapi = request(app, Method::GET, "/openapi.json", None, None, None, None).await;
    assert_eq!(openapi.status, StatusCode::OK);
    assert_eq!(openapi.json["openapi"], "3.1.0");
    for path in [
        "/api/v1/admin/auth/login",
        "/api/v1/admin/users/{id}",
        "/api/v1/admin/roles",
    ] {
        assert!(
            openapi.json["paths"][path].is_object(),
            "missing OpenAPI path: {path}"
        );
    }
    let unauthorized = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(unauthorized.status, StatusCode::UNAUTHORIZED);
    let invalid_cookie = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some("__Host-silva_session=not-a-session"),
        None,
        None,
    )
    .await;
    assert_eq!(invalid_cookie.status, StatusCode::UNAUTHORIZED);

    for origin in [
        None,
        Some("https://evil.example.test"),
        Some("https://admin.example.test/"),
    ] {
        let response = request(
            app,
            Method::POST,
            "/api/v1/admin/auth/login",
            Some(json!({"email":"admin@example.test","password":PASSWORD})),
            None,
            None,
            origin,
        )
        .await;
        assert_error(&response, StatusCode::FORBIDDEN, "origin_rejected");
    }
    let not_json = raw_request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some("email=admin@example.test"),
        Some("text/plain"),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_error(
        &not_json,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "unsupported_media_type",
    );
    let malformed = raw_request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some("{"),
        Some("application/json"),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_error(&malformed, StatusCode::BAD_REQUEST, "malformed_json");
    let oversized_body =
        json!({"email":"admin@example.test","password":"x".repeat(70 * 1024)}).to_string();
    let oversized = raw_request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(&oversized_body),
        Some("application/json"),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_error(
        &oversized,
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
    );

    // 账户不存在与密码错误必须返回相同的错误结构。
    let mut credential_errors = Vec::new();
    for (email, password) in [
        ("missing@example.test", PASSWORD),
        ("admin@example.test", "incorrect-password"),
    ] {
        let response = request(
            app,
            Method::POST,
            "/api/v1/admin/auth/login",
            Some(json!({"email":email,"password":password})),
            None,
            None,
            Some(ORIGIN),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{}",
            response.json
        );
        credential_errors.push((
            response.json["error"]["code"].clone(),
            response.json["error"]["message"].clone(),
        ));
    }
    assert_eq!(credential_errors[0], credential_errors[1]);

    let (admin_cookie, admin_csrf, admin_login) = login(app, "admin@example.test").await;
    let admin_id = admin_login["user"]["id"].as_str().unwrap();
    let me = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.json);
    assert_eq!(me.json["data"]["csrf_token"], admin_csrf);
    assert_eq!(me.json["data"]["user"]["id"], admin_id);
    assert_eq!(me.headers[header::CACHE_CONTROL], "no-store");
    let token = admin_cookie.split_once('=').unwrap().1;
    let stored: (String, String) = sqlx::query_as("SELECT s.token_hash, u.password_hash FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.id = $1 ORDER BY s.created_at DESC LIMIT 1")
        .bind(Uuid::parse_str(admin_id).unwrap()).fetch_one(pool).await.unwrap();
    assert_ne!(stored.0, token);
    assert!(stored.1.starts_with("$argon2id$"));
    assert!(!stored.1.contains(PASSWORD));

    let (other_cookie, other_csrf, _) = login(app, "admin@example.test").await;
    assert_ne!(admin_cookie, other_cookie);
    assert_ne!(admin_csrf, other_csrf);
    for csrf in [None, Some("invalid-csrf"), Some(other_csrf.as_str())] {
        let response = request(
            app,
            Method::POST,
            "/api/v1/admin/auth/logout",
            None,
            Some(&admin_cookie),
            csrf,
            Some(ORIGIN),
        )
        .await;
        assert_error(&response, StatusCode::FORBIDDEN, "csrf_rejected");
    }
    let origin_rejected = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/logout",
        None,
        Some(&admin_cookie),
        Some(&admin_csrf),
        None,
    )
    .await;
    assert_error(&origin_rejected, StatusCode::FORBIDDEN, "origin_rejected");

    let created = request(app, Method::POST, "/api/v1/admin/users", Some(json!({"email":"author@example.test","display_name":"Acceptance Author","password":PASSWORD,"roles":["author"]})), Some(&admin_cookie), Some(&admin_csrf), Some(ORIGIN)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let author_id = created.json["data"]["id"].as_str().unwrap();
    assert_eq!(Uuid::parse_str(author_id).unwrap().get_version_num(), 7);
    assert!(created.json["data"].get("password_hash").is_none());
    let duplicate = request(app, Method::POST, "/api/v1/admin/users", Some(json!({"email":"AUTHOR@example.test","display_name":"Duplicate","password":PASSWORD,"roles":["author"]})), Some(&admin_cookie), Some(&admin_csrf), Some(ORIGIN)).await;
    assert_error(&duplicate, StatusCode::CONFLICT, "email_in_use");
    for (body, field, code) in [
        (
            json!({"email":"bad@example.test","display_name":"Invalid","password":"short","roles":["author"]}),
            "password",
            "out_of_range",
        ),
        (
            json!({"email":"bad@example.test","display_name":"Invalid","password":PASSWORD,"roles":["custom"]}),
            "roles[0]",
            "invalid_value",
        ),
        (
            json!({"email":"bad@example.test","display_name":"Invalid","password":PASSWORD,"roles":["author","author"]}),
            "roles[1]",
            "duplicate",
        ),
        (
            json!({"email":"no-at-sign","display_name":"Invalid","password":PASSWORD,"roles":["author"]}),
            "email",
            "invalid_format",
        ),
        (
            json!({"email":"bad@example.test","password":PASSWORD,"roles":["author"]}),
            "display_name",
            "required",
        ),
        (
            json!({"email":"bad@example.test","display_name":"Invalid","password":PASSWORD,"roles":["author"],"is_admin":true}),
            "is_admin",
            "unknown_field",
        ),
        (
            json!({"email":"bad@example.test","display_name":"Invalid","password":PASSWORD,"roles":["author", 1]}),
            "roles[1]",
            "invalid_type",
        ),
    ] {
        let invalid = request(
            app,
            Method::POST,
            "/api/v1/admin/users",
            Some(body),
            Some(&admin_cookie),
            Some(&admin_csrf),
            Some(ORIGIN),
        )
        .await;
        assert_field_error(&invalid, field, code);
    }
    let (author_cookie, author_csrf, _) = login(app, "author@example.test").await;
    let (author_cookie2, _, _) = login(app, "author@example.test").await;

    // 修改自己的密码：校验当前密码，保留本会话并撤销其他会话。
    const NEW_PASSWORD: &str = "rotated-password-2026";
    for (body, field, code) in [
        (
            json!({"current_password":"wrong-password-0000","new_password":NEW_PASSWORD}),
            "current_password",
            "incorrect",
        ),
        (
            json!({"current_password":PASSWORD,"new_password":PASSWORD}),
            "new_password",
            "unchanged",
        ),
        (
            json!({"current_password":PASSWORD,"new_password":"short"}),
            "new_password",
            "out_of_range",
        ),
    ] {
        let response = request(
            app,
            Method::POST,
            "/api/v1/admin/auth/password",
            Some(body),
            Some(&author_cookie),
            Some(&author_csrf),
            Some(ORIGIN),
        )
        .await;
        assert_field_error(&response, field, code);
    }
    let missing_csrf = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/password",
        Some(json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD})),
        Some(&author_cookie),
        None,
        Some(ORIGIN),
    )
    .await;
    assert_error(&missing_csrf, StatusCode::FORBIDDEN, "csrf_rejected");
    let changed = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/password",
        Some(json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD})),
        Some(&author_cookie),
        Some(&author_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.json);
    let kept = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&author_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(kept.status, StatusCode::OK, "current session must survive");
    let other = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&author_cookie2),
        None,
        None,
    )
    .await;
    assert_eq!(
        other.status,
        StatusCode::UNAUTHORIZED,
        "other sessions must be revoked"
    );
    let change_audit: Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_logs WHERE action = 'auth.password_change' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(change_audit["revoked_sessions"], 1);
    for (password, status) in [
        (PASSWORD, StatusCode::UNAUTHORIZED),
        (NEW_PASSWORD, StatusCode::OK),
    ] {
        let response = request(
            app,
            Method::POST,
            "/api/v1/admin/auth/login",
            Some(json!({"email":"author@example.test","password":password})),
            None,
            None,
            Some(ORIGIN),
        )
        .await;
        assert_eq!(response.status, status, "{}", response.json);
    }

    // 管理员重置密码：撤销目标账户全部会话，审计不含密码或哈希。
    let reset = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"password":PASSWORD})),
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(reset.status, StatusCode::OK, "{}", reset.json);
    let after_reset = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&author_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(
        after_reset.status,
        StatusCode::UNAUTHORIZED,
        "reset must revoke every session"
    );
    let reset_audit: Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_logs WHERE action = 'users.update' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(reset_audit["fields"], json!(["password"]));
    assert!(!reset_audit.to_string().contains(PASSWORD));
    let short_reset = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"password":"short"})),
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_field_error(&short_reset, "password", "out_of_range");
    let (author_cookie, author_csrf, _) = login(app, "author@example.test").await;
    let (author_cookie2, _, _) = login(app, "author@example.test").await;
    for path in [
        "/api/v1/admin/users",
        "/api/v1/admin/roles",
        "/api/v1/admin/audit-logs",
        "/api/v1/admin/system/health",
    ] {
        let response = request(
            app,
            Method::GET,
            path,
            None,
            Some(&author_cookie),
            None,
            None,
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{path}: {}",
            response.json
        );
    }
    let forbidden_create = request(app, Method::POST, "/api/v1/admin/users", Some(json!({"email":"intruder@example.test","display_name":"No","password":PASSWORD,"roles":["admin"]})), Some(&author_cookie), Some(&author_csrf), Some(ORIGIN)).await;
    assert_eq!(
        forbidden_create.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden_create.json
    );

    let users = request(
        app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(users.status, StatusCode::OK, "{}", users.json);
    let pagination = &users.json["meta"]["pagination"];
    assert_eq!(pagination["page"], 1);
    assert_eq!(pagination["per_page"], 20);
    assert_eq!(pagination["total"], 2);
    assert_eq!(pagination["total_pages"], 1);
    assert_eq!(users.json["data"].as_array().unwrap().len(), 2);
    let healthy = request(
        app,
        Method::GET,
        "/api/v1/admin/system/health",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(healthy.status, StatusCode::OK, "{}", healthy.json);
    assert_eq!(healthy.json["data"]["database"], "up");
    for (query, field, code) in [
        ("page=0", "page", "out_of_range"),
        ("per_page=0", "per_page", "out_of_range"),
        ("per_page=101", "per_page", "out_of_range"),
        ("page=not-a-number", "page", "invalid_type"),
        ("page=99999999999", "page", "out_of_range"),
        ("page=1&page=2", "page", "duplicate"),
    ] {
        let response = request(
            app,
            Method::GET,
            &format!("/api/v1/admin/users?{query}"),
            None,
            Some(&admin_cookie),
            None,
            None,
        )
        .await;
        assert_field_error(&response, field, code);
    }

    for bad_id in [
        "550e8400-e29b-41d4-a716-446655440000",
        "019535d9-3df7-79fb-7466-fa907fa17f9e",
        "019535d93df779fbb466fa907fa17f9e",
        "not-an-id",
    ] {
        let response = request(
            app,
            Method::PATCH,
            &format!("/api/v1/admin/users/{bad_id}"),
            Some(json!({"display_name":"Rejected ID"})),
            Some(&admin_cookie),
            Some(&admin_csrf),
            Some(ORIGIN),
        )
        .await;
        assert_field_error(&response, "id", "invalid_format");
    }
    let roles = request(
        app,
        Method::GET,
        "/api/v1/admin/roles",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(roles.status, StatusCode::OK, "{}", roles.json);
    let role_keys: Vec<_> = roles.json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|role| role["key"].as_str().unwrap())
        .collect();
    for role in ["author", "editor", "admin"] {
        assert!(role_keys.contains(&role));
    }
    for role in roles.json["data"].as_array().unwrap() {
        assert_eq!(
            Uuid::parse_str(role["id"].as_str().unwrap())
                .unwrap()
                .get_version_num(),
            7
        );
        assert!(role["name"].is_string());
        assert!(!role["permissions"].as_array().unwrap().is_empty());
    }

    // 角色变更无需重新登录即可生效，也允许实现直接撤销旧会话。
    let promoted = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"roles":["admin"]})),
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(promoted.status, StatusCode::OK, "{}", promoted.json);
    let current = request(
        app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        Some(&author_cookie),
        None,
        None,
    )
    .await;
    assert!(
        matches!(current.status, StatusCode::OK | StatusCode::UNAUTHORIZED),
        "{}",
        current.json
    );
    let demoted = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"roles":["author"]})),
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(demoted.status, StatusCode::OK, "{}", demoted.json);
    let denied = request(
        app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        Some(&author_cookie),
        None,
        None,
    )
    .await;
    assert!(
        matches!(
            denied.status,
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED
        ),
        "{}",
        denied.json
    );

    // 唯一启用的管理员不能被停用或移除 admin 角色，失败时整笔修改回滚且不写审计。
    let admin_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'admin@example.test'")
            .fetch_one(pool)
            .await
            .unwrap();
    let audits_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs")
        .fetch_one(pool)
        .await
        .unwrap();
    for body in [
        json!({"status":"disabled"}),
        json!({"roles":["editor"]}),
        json!({"display_name":"Renamed","roles":["author"]}),
    ] {
        let response = request(
            app,
            Method::PATCH,
            &format!("/api/v1/admin/users/{admin_id}"),
            Some(body),
            Some(&admin_cookie),
            Some(&admin_csrf),
            Some(ORIGIN),
        )
        .await;
        assert_error(&response, StatusCode::CONFLICT, "last_admin_required");
    }
    let (admin_status, admin_name): (String, String) =
        sqlx::query_as("SELECT status, display_name FROM users WHERE id = $1")
            .bind(admin_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(admin_status, "active");
    assert_ne!(admin_name, "Renamed", "rejected update must roll back");
    let audits_after: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(audits_before, audits_after);
    let still_admin = request(
        app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(still_admin.status, StatusCode::OK, "{}", still_admin.json);
    let audit_metadata: Value = sqlx::query_scalar(
        "SELECT metadata FROM audit_logs WHERE action = 'users.update' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(audit_metadata["fields"], json!(["roles"]));
    assert_eq!(audit_metadata["roles"], json!(["author"]));
    let (audit_ip, audit_agent): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT host(ip_address), user_agent FROM audit_logs WHERE action = 'auth.login' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(audit_ip.as_deref(), Some("2001:db8::7"));
    assert_eq!(audit_agent.as_deref(), Some(TEST_USER_AGENT));

    let disabled = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"status":"disabled"})),
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::OK, "{}", disabled.json);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(Uuid::parse_str(author_id).unwrap())
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        remaining, 0,
        "disabling an account must remove every session"
    );
    for cookie in [&author_cookie, &author_cookie2] {
        let response = request(
            app,
            Method::GET,
            "/api/v1/admin/auth/me",
            None,
            Some(cookie),
            None,
            None,
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{}",
            response.json
        );
    }
    let disabled_login = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email":"author@example.test","password":PASSWORD})),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_error(
        &disabled_login,
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
    );
    // 停用账户对外与密码错误一致，审计中区分原因并关联账户。
    let (disabled_resource, disabled_metadata): (Option<Uuid>, Value) = sqlx::query_as(
        "SELECT resource_id, metadata FROM audit_logs WHERE action = 'auth.login_failed' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(disabled_resource, Some(Uuid::parse_str(author_id).unwrap()));
    assert_eq!(disabled_metadata, json!({"reason":"account_disabled"}));
    let enabled = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"status":"active"})),
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(enabled.status, StatusCode::OK, "{}", enabled.json);
    assert_eq!(enabled.json["data"]["status"], "active");
    let old_session = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&author_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(
        old_session.status,
        StatusCode::UNAUTHORIZED,
        "re-enabling must not resurrect revoked sessions"
    );
    let _ = login(app, "author@example.test").await;

    sqlx::query(
        "UPDATE sessions SET expires_at = now() - interval '1 second' WHERE token_hash <> $1",
    )
    .bind(&stored.0)
    .execute(pool)
    .await
    .unwrap();
    let expired = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&other_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED);
    let audit = request(
        app,
        Method::GET,
        "/api/v1/admin/audit-logs",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(audit.status, StatusCode::OK, "{}", audit.json);
    assert!(!audit.json["data"].as_array().unwrap().is_empty());
    let first_audit = &audit.json["data"][0];
    assert!(first_audit["metadata"].is_object());
    assert!(first_audit.get("ip_address").is_some());
    let serialized_audit = audit.json.to_string();
    for secret in [PASSWORD, token, &admin_csrf] {
        assert!(
            !serialized_audit.contains(secret),
            "audit logs must not expose credentials"
        );
    }

    let unknown = request(
        app,
        Method::GET,
        "/api/v1/admin/missing",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_error(&unknown, StatusCode::NOT_FOUND, "not_found");
    let method = request(
        app,
        Method::DELETE,
        "/api/v1/admin/auth/me",
        None,
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_error(
        &method,
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
    );
    let logout = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/logout",
        None,
        Some(&admin_cookie),
        Some(&admin_csrf),
        Some(ORIGIN),
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT, "{}", logout.json);
    assert_eq!(logout.json, Value::Null);
    let after_logout = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&admin_cookie),
        None,
        None,
    )
    .await;
    assert_eq!(after_logout.status, StatusCode::UNAUTHORIZED);
}

async fn run_rate_limits(pool: &PgPool, database_url: &str) {
    let mut api_config = config(database_url.to_owned());
    api_config.api_rate_per_minute = 1;
    let api_limited = silva_api::build_router(api_config, pool.clone())
        .await
        .unwrap();
    let first = request(
        &api_limited,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);
    let second = request(
        &api_limited,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_error(&second, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert_eq!(second.headers[header::RETRY_AFTER], "60");

    let mut login_config = config(database_url.to_owned());
    login_config.login_rate_per_minute = 1;
    let login_limited = silva_api::build_router(login_config, pool.clone())
        .await
        .unwrap();
    let first = request(
        &login_limited,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email":"missing@example.test","password":PASSWORD})),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);
    let second = request(
        &login_limited,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email":"missing@example.test","password":PASSWORD})),
        None,
        None,
        Some(ORIGIN),
    )
    .await;
    assert_error(&second, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert_eq!(second.headers[header::RETRY_AFTER], "60");
    // 登录限流不得在 API 总额度用尽前阻止其他 API 请求。
    let me = request(
        &login_limited,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

async fn login_attempt(app: &Router, email: &str, password: &str) -> Response {
    request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email": email, "password": password})),
        None,
        None,
        Some(ORIGIN),
    )
    .await
}

fn assert_locked(response: &Response, max_seconds: u64) {
    assert_error(response, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    let seconds: u64 = response.headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (1..=max_seconds).contains(&seconds),
        "Retry-After 应为剩余锁定秒数：{seconds}"
    );
}

/// 按账户的连续失败锁定：阈值调小为 3 次、锁定 2 秒，验证锁定、审计、到期和成功清零。
async fn run_login_lockout(pool: &PgPool, database_url: &str) {
    const WRONG: &str = "wrong-password-0000";
    let mut lockout_config = config(database_url.to_owned());
    lockout_config.login_lockout_threshold = 3;
    lockout_config.login_lockout_duration = Duration::from_secs(2);
    let app = silva_api::build_router(lockout_config, pool.clone())
        .await
        .unwrap();
    let admin_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'admin@example.test'")
            .fetch_one(pool)
            .await
            .unwrap();
    // UUIDv7 按时间递增，以此筛选本段测试写入的审计。
    let baseline = ids::new_id();
    tokio::time::sleep(Duration::from_millis(5)).await;

    for _ in 0..3 {
        let response = login_attempt(&app, "admin@example.test", WRONG).await;
        assert_error(&response, StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    // 锁定期内即使密码正确也被拒绝。
    assert_locked(
        &login_attempt(&app, "admin@example.test", PASSWORD).await,
        2,
    );
    assert_locked(
        &login_attempt(&app, "ADMIN@example.test", PASSWORD).await,
        2,
    );

    // 不存在的账户按规范化邮箱同样计数和锁定，响应与存在的账户一致。
    for email in [
        " Ghost@Example.test",
        "ghost@example.test ",
        "GHOST@example.test",
    ] {
        let response = login_attempt(&app, email, WRONG).await;
        assert_error(&response, StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    assert_locked(
        &login_attempt(&app, "ghost@example.test", PASSWORD).await,
        2,
    );
    assert_locked(&login_attempt(&app, "ghost@example.test", WRONG).await, 2);

    /// actor_user_id、resource_id、resource_type、metadata 与整行 JSON 文本。
    type FailedLoginRow = (Option<Uuid>, Option<Uuid>, String, Value, String);
    let rows: Vec<FailedLoginRow> = sqlx::query_as(
        "SELECT actor_user_id, resource_id, resource_type, metadata, row_to_json(a)::text FROM audit_logs a WHERE action = 'auth.login_failed' AND id > $1 ORDER BY id",
    )
    .bind(baseline)
    .fetch_all(pool)
    .await
    .unwrap();
    let summary: Vec<_> = rows
        .iter()
        .map(|(actor, resource, kind, metadata, _)| {
            assert_eq!(*actor, None);
            assert_eq!(kind, "user");
            (*resource, metadata["reason"].as_str().unwrap().to_owned())
        })
        .collect();
    let expected = |resource: Option<Uuid>| {
        let mut rows = vec![(resource, "invalid_credentials".to_owned()); 3];
        // 同一轮锁定只在首次被拒时记录一次。
        rows.push((resource, "locked".to_owned()));
        rows
    };
    assert_eq!(summary, [expected(Some(admin_id)), expected(None)].concat());
    for (_, _, _, metadata, row) in &rows {
        assert_eq!(metadata.as_object().unwrap().len(), 1, "{metadata}");
        for secret in ["example.test", PASSWORD, WRONG] {
            assert!(
                !row.to_lowercase().contains(&secret.to_lowercase()),
                "失败登录审计不得包含邮箱或密码：{row}"
            );
        }
    }

    // 锁定到期后恢复登录。
    tokio::time::sleep(Duration::from_millis(2100)).await;
    login(&app, "admin@example.test").await;

    // 成功登录清零：两次失败、成功、再两次失败后仍可登录。
    for _ in 0..2 {
        let response = login_attempt(&app, "admin@example.test", WRONG).await;
        assert_error(&response, StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    login(&app, "admin@example.test").await;
    for _ in 0..2 {
        let response = login_attempt(&app, "admin@example.test", WRONG).await;
        assert_error(&response, StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    login(&app, "admin@example.test").await;
}
