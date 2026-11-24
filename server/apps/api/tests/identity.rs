//! 使用真实 PostgreSQL 的验收测试，通过 `cargo test -p silva-api --test identity -- --ignored` 显式执行。
mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use silva_shared::{
    config::{Config, JwtSecret},
    ids,
};
use sqlx::{Connection, PgConnection, PgPool, postgres::PgConnectOptions};
use std::{net::SocketAddr, str::FromStr, time::Duration};
use tower::ServiceExt;
use uuid::{Uuid, Variant};

const PASSWORD: &str = "acceptance-password-2026";
const TEST_USER_AGENT: &str = "silva-acceptance/1.0";
const TEST_PEER: &str = "[2001:db8::7]:40000";
/// 测试使用固定签名密钥，使同一数据库上构建的多个路由互认令牌，并能构造伪造令牌。
const JWT_SECRET: &[u8] = b"acceptance-jwt-secret-0123456789abcdef";

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    json: Value,
}

/// 发送 JSON 请求；`token` 为 access token，按 `Authorization: Bearer <token>` 发送。
async fn request(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> Response {
    let body = body.map(|value| value.to_string());
    let authorization = token.map(|token| format!("Bearer {token}"));
    raw_request(
        app,
        method,
        path,
        body.as_deref(),
        Some("application/json"),
        authorization.as_deref(),
    )
    .await
}

/// 发送原始请求；`authorization` 为完整的 Authorization 请求头值，便于构造格式错误的请求头。
async fn raw_request(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    authorization: Option<&str>,
) -> Response {
    let mut builder = Request::builder().method(method.clone()).uri(path);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    if let Some(value) = authorization {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    send(app, builder, body).await
}

async fn send(app: &Router, builder: axum::http::request::Builder, body: Option<&str>) -> Response {
    let method = builder.method_ref().unwrap().clone();
    let path = builder.uri_ref().unwrap().to_string();
    // 模拟真实连接的 peer 地址和浏览器 UA，验证审计来源信息。
    let builder = builder
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
    support::assert_response(&method, &path, status, &headers, &bytes);
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
    // 请求 ID 只在响应头中返回，错误正文不重复携带。
    assert!(response.json["error"].get("request_id").is_none());
    let request_id = response.headers["x-request-id"].to_str().unwrap();
    let id = Uuid::parse_str(request_id).unwrap();
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(id.get_variant(), Variant::RFC4122);
    if status == StatusCode::UNAUTHORIZED {
        let challenge = response.headers[header::WWW_AUTHENTICATE].to_str().unwrap();
        assert!(challenge.starts_with("Bearer"), "{challenge}");
    }
}

/// 断言 401 `unauthorized`。
fn assert_unauthorized(response: &Response) {
    assert_error(response, StatusCode::UNAUTHORIZED, "unauthorized");
}

/// 解析 `*_expires_at`：必须是带 `+08:00` 偏移、精确到秒的 RFC 3339 时间。
fn expires_at(data: &Value, field: &str) -> chrono::DateTime<chrono::Utc> {
    let text = data[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} 应为字符串：{data}"));
    assert!(text.ends_with("+08:00"), "{field} 应为北京时间：{text}");
    let parsed = chrono::DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(parsed.timestamp_subsec_nanos(), 0, "{field} 应精确到秒");
    parsed
}

/// 校验令牌对字段并返回 (access_token, refresh_token)：正文只含两个令牌及其过期时间，
/// 过期时间与令牌中的 `exp` 声明一致。
fn token_pair(data: &Value, access_ttl: i64) -> (String, String) {
    let mut keys: Vec<_> = data.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "access_token",
            "access_token_expires_at",
            "refresh_token",
            "refresh_token_expires_at"
        ],
        "{data}"
    );
    let access_expires = expires_at(data, "access_token_expires_at");
    let refresh_expires = expires_at(data, "refresh_token_expires_at");
    let now = chrono::Utc::now().timestamp();
    assert!(
        (access_expires.timestamp() - now - access_ttl).abs() <= 2,
        "{data}"
    );
    assert!(
        (refresh_expires.timestamp() - now - 3600).abs() <= 2,
        "{data}"
    );
    let access = data["access_token"].as_str().unwrap().to_owned();
    let refresh = data["refresh_token"].as_str().unwrap().to_owned();
    assert_eq!(access.split('.').count(), 3, "access token 应为 JWT");
    assert_eq!(refresh.split('.').count(), 3, "refresh token 应为 JWT");
    assert_ne!(access, refresh);
    assert_eq!(jwt_payload(&access)["exp"], access_expires.timestamp());
    assert_eq!(jwt_payload(&refresh)["exp"], refresh_expires.timestamp());
    (access, refresh)
}

/// 登录并返回 (access_token, refresh_token, 当前账户详情)；账户详情来自登录后的 `GET /users/info`。
async fn login(app: &Router, email: &str) -> (String, String, Value) {
    let response = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email": email, "password": PASSWORD})),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.json);
    assert_eq!(response.headers[header::CACHE_CONTROL], "no-store");
    assert!(response.headers.get(header::SET_COOKIE).is_none());
    let (access, refresh) = token_pair(&response.json["data"], 900);
    let info = me(app, &access).await;
    assert_eq!(info.status, StatusCode::OK, "{}", info.json);
    let info = info.json["data"].clone();
    assert!(info["id"].is_string());
    assert!(info["roles"].is_array());
    assert!(info["permissions"].is_array());
    for field in ["created_at", "updated_at", "last_login_at"] {
        assert_timestamp(&info[field]);
    }
    (access, refresh, info)
}

/// 调用刷新接口。
async fn refresh(app: &Router, refresh_token: &str) -> Response {
    request(
        app,
        Method::POST,
        "/api/v1/admin/auth/refresh",
        Some(json!({"refresh_token": refresh_token})),
        None,
    )
    .await
}

async fn me(app: &Router, access: &str) -> Response {
    request(
        app,
        Method::GET,
        "/api/v1/admin/users/info",
        None,
        Some(access),
    )
    .await
}

/// 解码 JWT 载荷（不校验签名），用于检查声明与构造伪造令牌。
fn jwt_payload(token: &str) -> Value {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let part = token.split('.').nth(1).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
}

/// 用指定密钥与算法签发任意声明的令牌。
fn forge(claims: &Value, secret: &[u8], algorithm: jsonwebtoken::Algorithm) -> String {
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(algorithm),
        claims,
        &jsonwebtoken::EncodingKey::from_secret(secret),
    )
    .unwrap()
}

/// 由数据库计算令牌的 SHA-256 十六进制哈希，与服务端保存方式一致。
async fn sha256_hex(pool: &PgPool, token: &str) -> String {
    sqlx::query_scalar("SELECT encode(sha256(convert_to($1, 'UTF8')), 'hex')")
        .bind(token)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 断言令牌原文不出现在会话表与审计表的任何列中。
async fn assert_tokens_not_stored(pool: &PgPool, tokens: &[&str]) {
    for token in tokens {
        let found: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM sessions s WHERE row_to_json(s)::text LIKE '%' || $1 || '%') + (SELECT count(*) FROM audit_logs a WHERE row_to_json(a)::text LIKE '%' || $1 || '%')",
        )
        .bind(token)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(found, 0, "数据库不得保存令牌原文");
    }
}

/// 同时提交两次相同的 refresh token。
async fn futures_join(app: &Router, refresh_token: &str) -> [Response; 2] {
    let (first, second) = tokio::join!(refresh(app, refresh_token), refresh(app, refresh_token));
    [first, second]
}

/// 伪造、篡改、错用与过期令牌的拒绝规则。
async fn run_token_validation(app: &Router, access: &str, refresh_token: &str, claims: &Value) {
    use jsonwebtoken::Algorithm;
    let now = chrono::Utc::now().timestamp();
    let fresh = |changes: Value| {
        let mut claims = claims.clone();
        claims["jti"] = json!(ids::new_id());
        claims["iat"] = json!(now);
        claims["exp"] = json!(now + 900);
        for (key, value) in changes.as_object().unwrap() {
            claims[key] = value.clone();
        }
        claims
    };
    // 正确密钥与声明签发的令牌可用，说明以下拒绝来自被修改的那一项。
    let control = forge(&fresh(json!({})), JWT_SECRET, Algorithm::HS256);
    assert_eq!(me(app, &control).await.status, StatusCode::OK);

    let parts: Vec<&str> = access.split('.').collect();
    let flipped = {
        let mut signature = parts[2].to_owned();
        let first = signature.remove(0);
        signature.insert(0, if first == 'A' { 'B' } else { 'A' });
        signature
    };
    let tampered_signature = format!("{}.{}.{flipped}", parts[0], parts[1]);
    let tampered_payload = {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let mut body = claims.clone();
        body["sub"] = json!(ids::new_id());
        format!(
            "{}.{}.{}",
            parts[0],
            URL_SAFE_NO_PAD.encode(body.to_string()),
            parts[2]
        )
    };
    let alg_none = {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#),
            URL_SAFE_NO_PAD.encode(fresh(json!({})).to_string())
        )
    };
    let rejected = [
        ("篡改签名", tampered_signature),
        ("篡改载荷", tampered_payload),
        (
            "其他密钥",
            forge(
                &fresh(json!({})),
                b"another-secret-0123456789abcdefghij",
                Algorithm::HS256,
            ),
        ),
        (
            "错误 iss",
            forge(
                &fresh(json!({"iss": "someone-else"})),
                JWT_SECRET,
                Algorithm::HS256,
            ),
        ),
        (
            "错误 aud",
            forge(
                &fresh(json!({"aud": "silva-public"})),
                JWT_SECRET,
                Algorithm::HS256,
            ),
        ),
        ("alg none", alg_none),
        (
            "HS512",
            forge(&fresh(json!({})), JWT_SECRET, Algorithm::HS512),
        ),
        ("refresh 当 access", refresh_token.to_owned()),
        (
            "不存在的会话",
            forge(
                &fresh(json!({"sid": ids::new_id()})),
                JWT_SECRET,
                Algorithm::HS256,
            ),
        ),
        (
            "会话与账户不符",
            forge(
                &fresh(json!({"sub": ids::new_id()})),
                JWT_SECRET,
                Algorithm::HS256,
            ),
        ),
        (
            "缺少 typ",
            forge(
                &{
                    let mut claims = fresh(json!({}));
                    claims.as_object_mut().unwrap().remove("typ");
                    claims
                },
                JWT_SECRET,
                Algorithm::HS256,
            ),
        ),
    ];
    for (case, token) in &rejected {
        let response = me(app, token).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{case}");
        assert_unauthorized(&response);
    }

    // 签名有效但已过期的 access token 返回 token_expired，提示客户端刷新。
    let expired = forge(
        &fresh(json!({"iat": now - 901, "exp": now - 1})),
        JWT_SECRET,
        Algorithm::HS256,
    );
    let response = me(app, &expired).await;
    assert_error(&response, StatusCode::UNAUTHORIZED, "token_expired");
    let challenge = response.headers[header::WWW_AUTHENTICATE].to_str().unwrap();
    assert!(
        challenge.contains(r#"error="invalid_token""#),
        "{challenge}"
    );
    // 过期但签名错误的令牌仍按无效处理，不泄露过期信息。
    let forged_expired = forge(
        &fresh(json!({"iat": now - 901, "exp": now - 1})),
        b"another-secret-0123456789abcdefghij",
        Algorithm::HS256,
    );
    assert_unauthorized(&me(app, &forged_expired).await);

    // 刷新接口只接受 refresh token。
    for token in [access, "not-a-jwt", control.as_str()] {
        assert_unauthorized(&refresh(app, token).await);
    }
    let expired_refresh = forge(
        &fresh(json!({"typ": "refresh", "iat": now - 3601, "exp": now - 1})),
        JWT_SECRET,
        Algorithm::HS256,
    );
    assert_unauthorized(&refresh(app, &expired_refresh).await);
    let missing = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/refresh",
        Some(json!({})),
        None,
    )
    .await;
    assert_field_error(&missing, "refresh_token", "required");

    // 重复的 Authorization 请求头即使都有效也拒绝。
    let duplicate = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/admin/users/info")
        .header(header::AUTHORIZATION, format!("Bearer {access}"))
        .header(header::AUTHORIZATION, format!("Bearer {access}"));
    assert_unauthorized(&send(app, duplicate, None).await);
    assert_eq!(me(app, access).await.status, StatusCode::OK);
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
        let tokens = silva_infrastructure::TokenSettings {
            secret: JwtSecret::new(JWT_SECRET),
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(3600),
        };
        let service = silva_infrastructure::identity_service(test_pool.clone(), &tokens)
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
        run_account_queries(&app).await;
        run_audit_logs(&app, &test_pool).await;
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
    let live = request(app, Method::GET, "/health/live", None, None).await;
    assert_eq!(live.status, StatusCode::OK, "{}", live.json);
    assert_eq!(live.json["status"], "ok");
    // 缺失或格式错误的 Authorization 请求头一律 401 unauthorized，并带 Bearer 质询。
    assert_unauthorized(&request(app, Method::GET, "/api/v1/admin/users/info", None, None).await);
    for authorization in [
        "",
        "Bearer",
        "Bearer ",
        "Bearer  x.y.z",
        "Bearer a b",
        "Basic YWRtaW46c2VjcmV0",
        "Token x.y.z",
        "x.y.z",
        "Bearer not-a-jwt",
        "Bearer a.b.c",
    ] {
        let response = raw_request(
            app,
            Method::GET,
            "/api/v1/admin/users/info",
            None,
            None,
            Some(authorization),
        )
        .await;
        assert_unauthorized(&response);
    }

    let not_json = raw_request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some("email=admin@example.test"),
        Some("text/plain"),
        None,
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

    let (admin_access, admin_refresh, admin_login) = login(app, "admin@example.test").await;
    let admin_id = admin_login["id"].as_str().unwrap();
    let me_response = me(app, &admin_access).await;
    assert_eq!(me_response.status, StatusCode::OK, "{}", me_response.json);
    assert_eq!(me_response.json["data"]["id"], admin_id);
    // 账户详情只由 /users/info 提供，旧的 /auth/me 已移除。
    let removed = request(
        app,
        Method::GET,
        "/api/v1/admin/auth/me",
        None,
        Some(&admin_access),
    )
    .await;
    assert_error(&removed, StatusCode::NOT_FOUND, "not_found");
    assert_eq!(me_response.headers[header::CACHE_CONTROL], "no-store");
    assert!(me_response.headers.get("x-csrf-token").is_none());
    // access token 只含账户与会话标识，sessions 只保存 refresh token 的 SHA-256 哈希。
    let claims = jwt_payload(&admin_access);
    assert_eq!(claims["sub"], admin_id);
    assert_eq!(claims["iss"], "silva");
    assert_eq!(claims["aud"], "silva-admin");
    assert_eq!(claims["typ"], "access");
    let (admin_session, admin_hash, password_hash): (Uuid, String, String) = sqlx::query_as(
        "SELECT s.id, s.token_hash, u.password_hash FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.id = $1 ORDER BY s.created_at DESC LIMIT 1",
    )
    .bind(Uuid::parse_str(admin_id).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(claims["sid"], admin_session.to_string());
    assert_eq!(admin_hash, sha256_hex(pool, &admin_refresh).await);
    assert!(password_hash.starts_with("$argon2id$"));
    assert!(!password_hash.contains(PASSWORD));
    assert_tokens_not_stored(pool, &[&admin_access, &admin_refresh]).await;

    run_token_validation(app, &admin_access, &admin_refresh, &claims).await;

    // 刷新轮换：新令牌可用，旧 refresh token 立即失效；重放旧 refresh token 撤销整个会话。
    let (other_access, other_refresh, _) = login(app, "admin@example.test").await;
    assert_ne!(admin_access, other_access);
    assert_ne!(admin_refresh, other_refresh);
    let rotated = refresh(app, &other_refresh).await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.json);
    assert_eq!(rotated.headers[header::CACHE_CONTROL], "no-store");
    let (rotated_access, rotated_refresh) = token_pair(&rotated.json["data"], 900);
    assert_ne!(rotated_refresh, other_refresh);
    assert_eq!(
        jwt_payload(&rotated_access)["sid"],
        jwt_payload(&other_access)["sid"],
        "刷新沿用同一会话"
    );
    assert_eq!(me(app, &rotated_access).await.status, StatusCode::OK);
    let rotated_audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'auth.refresh%'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(rotated_audits, 0, "正常刷新不写审计");
    assert_tokens_not_stored(pool, &[&other_refresh, &rotated_refresh, &rotated_access]).await;
    let replay = refresh(app, &other_refresh).await;
    assert_unauthorized(&replay);
    for access in [&other_access, &rotated_access] {
        assert_unauthorized(&me(app, access).await);
    }
    assert_unauthorized(&refresh(app, &rotated_refresh).await);
    let reuse_audits: Vec<(Option<Uuid>, Option<Uuid>, Value, String)> = sqlx::query_as(
        "SELECT actor_user_id, resource_id, metadata, row_to_json(a)::text FROM audit_logs a WHERE action = 'auth.refresh_reuse'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(reuse_audits.len(), 1, "重放只记录一次审计");
    let (actor, resource, metadata, row) = &reuse_audits[0];
    assert_eq!(*actor, None);
    assert_eq!(*resource, Some(Uuid::parse_str(admin_id).unwrap()));
    assert_eq!(*metadata, json!({}));
    for token in [
        &other_access,
        &other_refresh,
        &rotated_access,
        &rotated_refresh,
    ] {
        assert!(!row.contains(token.as_str()), "审计不得包含令牌");
    }
    // 其他会话不受影响。
    assert_eq!(me(app, &admin_access).await.status, StatusCode::OK);

    // 同一 refresh token 并发提交只有一次成功。
    let (_, concurrent_refresh, _) = login(app, "admin@example.test").await;
    let attempts = futures_join(app, &concurrent_refresh).await;
    let succeeded = attempts
        .iter()
        .filter(|response| response.status == StatusCode::OK)
        .count();
    assert_eq!(succeeded, 1, "并发刷新只能成功一次");
    for response in attempts
        .iter()
        .filter(|response| response.status != StatusCode::OK)
    {
        assert_unauthorized(response);
    }
    // 落败的一次无法与重放区分，按重放撤销该会话，成功换得的新令牌随之失效。
    for response in attempts
        .iter()
        .filter(|response| response.status == StatusCode::OK)
    {
        let access = response.json["data"]["access_token"].as_str().unwrap();
        assert_unauthorized(&me(app, access).await);
    }

    let created = request(app, Method::POST, "/api/v1/admin/users", Some(json!({"email":"author@example.test","display_name":"Acceptance Author","password":PASSWORD,"roles":["author"]})), Some(&admin_access)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let author_id = created.json["data"]["id"].as_str().unwrap();
    assert_eq!(Uuid::parse_str(author_id).unwrap().get_version_num(), 7);
    assert!(created.json["data"].get("password_hash").is_none());
    let duplicate = request(app, Method::POST, "/api/v1/admin/users", Some(json!({"email":"AUTHOR@example.test","display_name":"Duplicate","password":PASSWORD,"roles":["author"]})), Some(&admin_access)).await;
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
            Some(&admin_access),
        )
        .await;
        assert_field_error(&invalid, field, code);
    }
    let (author_access, author_refresh, _) = login(app, "author@example.test").await;
    let (author_access2, author_refresh2, _) = login(app, "author@example.test").await;

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
            Some(&author_access),
        )
        .await;
        assert_field_error(&response, field, code);
    }
    let anonymous_change = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/password",
        Some(json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD})),
        None,
    )
    .await;
    assert_unauthorized(&anonymous_change);
    let changed = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/password",
        Some(json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD})),
        Some(&author_access),
    )
    .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.json);
    let kept = request(
        app,
        Method::GET,
        "/api/v1/admin/users/info",
        None,
        Some(&author_access),
    )
    .await;
    assert_eq!(kept.status, StatusCode::OK, "current session must survive");
    let other = request(
        app,
        Method::GET,
        "/api/v1/admin/users/info",
        None,
        Some(&author_access2),
    )
    .await;
    assert_eq!(
        other.status,
        StatusCode::UNAUTHORIZED,
        "other sessions must be revoked"
    );
    assert_unauthorized(&refresh(app, &author_refresh2).await);
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
        Some(&admin_access),
    )
    .await;
    assert_eq!(reset.status, StatusCode::OK, "{}", reset.json);
    let after_reset = request(
        app,
        Method::GET,
        "/api/v1/admin/users/info",
        None,
        Some(&author_access),
    )
    .await;
    assert_eq!(
        after_reset.status,
        StatusCode::UNAUTHORIZED,
        "reset must revoke every session"
    );
    assert_unauthorized(&refresh(app, &author_refresh).await);
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
        Some(&admin_access),
    )
    .await;
    assert_field_error(&short_reset, "password", "out_of_range");
    let (author_access, author_refresh, _) = login(app, "author@example.test").await;
    let (author_access2, _, _) = login(app, "author@example.test").await;
    for path in [
        "/api/v1/admin/users",
        "/api/v1/admin/roles",
        "/api/v1/admin/audit-logs",
    ] {
        let response = request(app, Method::GET, path, None, Some(&author_access)).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{path}: {}",
            response.json
        );
    }
    let forbidden_create = request(app, Method::POST, "/api/v1/admin/users", Some(json!({"email":"intruder@example.test","display_name":"No","password":PASSWORD,"roles":["admin"]})), Some(&author_access)).await;
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
        Some(&admin_access),
    )
    .await;
    assert_eq!(users.status, StatusCode::OK, "{}", users.json);
    let pagination = &users.json["meta"]["pagination"];
    assert_eq!(pagination["page"], 1);
    assert_eq!(pagination["per_page"], 20);
    assert_eq!(pagination["total"], 2);
    assert_eq!(pagination["total_pages"], 1);
    assert_eq!(users.json["data"].as_array().unwrap().len(), 2);
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
            Some(&admin_access),
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
            Some(&admin_access),
        )
        .await;
        assert_field_error(&response, "id", "invalid_format");
    }
    let roles = request(
        app,
        Method::GET,
        "/api/v1/admin/roles",
        None,
        Some(&admin_access),
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

    // 角色变更无需重新登录即可对已签发的 access token 立即生效。
    let promoted = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"roles":["admin"]})),
        Some(&admin_access),
    )
    .await;
    assert_eq!(promoted.status, StatusCode::OK, "{}", promoted.json);
    let current = request(
        app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        Some(&author_access),
    )
    .await;
    assert_eq!(current.status, StatusCode::OK, "{}", current.json);
    let demoted = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{author_id}"),
        Some(json!({"roles":["author"]})),
        Some(&admin_access),
    )
    .await;
    assert_eq!(demoted.status, StatusCode::OK, "{}", demoted.json);
    let denied = request(
        app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        Some(&author_access),
    )
    .await;
    assert_error(&denied, StatusCode::FORBIDDEN, "forbidden");

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
            Some(&admin_access),
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
        Some(&admin_access),
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
        Some(&admin_access),
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
    for access in [&author_access, &author_access2] {
        assert_unauthorized(&me(app, access).await);
    }
    assert_unauthorized(&refresh(app, &author_refresh).await);
    let disabled_login = request(
        app,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email":"author@example.test","password":PASSWORD})),
        None,
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
        Some(&admin_access),
    )
    .await;
    assert_eq!(enabled.status, StatusCode::OK, "{}", enabled.json);
    assert_eq!(enabled.json["data"]["status"], "active");
    let old_session = request(
        app,
        Method::GET,
        "/api/v1/admin/users/info",
        None,
        Some(&author_access),
    )
    .await;
    assert_eq!(
        old_session.status,
        StatusCode::UNAUTHORIZED,
        "re-enabling must not resurrect revoked sessions"
    );
    let (expiring_access, expiring_refresh, _) = login(app, "author@example.test").await;

    // 服务端会话过期后，即使 JWT 本身尚未过期，access 与 refresh token 均失效。
    sqlx::query(
        "UPDATE sessions SET expires_at = now() - interval '1 second' WHERE token_hash <> $1",
    )
    .bind(&admin_hash)
    .execute(pool)
    .await
    .unwrap();
    assert_unauthorized(&me(app, &expiring_access).await);
    assert_unauthorized(&refresh(app, &expiring_refresh).await);
    let audit = request(
        app,
        Method::GET,
        "/api/v1/admin/audit-logs",
        None,
        Some(&admin_access),
    )
    .await;
    assert_eq!(audit.status, StatusCode::OK, "{}", audit.json);
    assert!(!audit.json["data"].as_array().unwrap().is_empty());
    let first_audit = &audit.json["data"][0];
    assert!(first_audit["metadata"].is_object());
    assert!(first_audit.get("ip_address").is_some());
    let serialized_audit = audit.json.to_string();
    for secret in [PASSWORD, &admin_access, &admin_refresh] {
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
        Some(&admin_access),
    )
    .await;
    assert_error(&unknown, StatusCode::NOT_FOUND, "not_found");
    let method = request(
        app,
        Method::DELETE,
        "/api/v1/admin/users/info",
        None,
        Some(&admin_access),
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
        Some(&admin_access),
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT, "{}", logout.json);
    assert_eq!(logout.json, Value::Null);
    let after_logout = request(
        app,
        Method::GET,
        "/api/v1/admin/users/info",
        None,
        Some(&admin_access),
    )
    .await;
    assert_unauthorized(&after_logout);
    // 退出后同一会话的 refresh token 也不能再换取新令牌；会话已不存在，不按重放记录。
    assert_unauthorized(&refresh(app, &admin_refresh).await);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE id = $1")
        .bind(admin_session)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "退出登录必须删除会话");
    let reuse_audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'auth.refresh_reuse'")
            .fetch_one(pool)
            .await
            .unwrap();
    // 只有轮换后重放与并发刷新中落败的一次各记录一条。
    assert_eq!(reuse_audits, 2);
    assert_tokens_not_stored(pool, &[&admin_access, &admin_refresh]).await;
}

/// 发起只读管理请求的便捷封装。
async fn get(app: &Router, path: &str, access: &str) -> Response {
    request(app, Method::GET, path, None, Some(access)).await
}

fn emails(response: &Response) -> Vec<String> {
    response.json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| user["email"].as_str().unwrap().to_owned())
        .collect()
}

fn assert_timestamp(value: &Value) {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("应为 RFC 3339 时间：{value}"));
    assert!(text.ends_with("+08:00"), "API 时间应为北京时间：{text}");
    chrono::DateTime::parse_from_rfc3339(text).unwrap();
}

/// 账户详情、最近登录时间、列表筛选与角色账户数。
async fn run_account_queries(app: &Router) {
    let (admin_access, _, admin_login) = login(app, "admin@example.test").await;
    assert_timestamp(&admin_login["last_login_at"]);
    let me = get(app, "/api/v1/admin/users/info", &admin_access).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.json);
    assert_eq!(
        me.json["data"]["last_login_at"],
        admin_login["last_login_at"]
    );

    // 依次创建：含 `_` 与 `%` 的账户、仅用于对照的近似账户、含反斜杠的账户、将被停用的多角色账户。
    let mut created = Vec::new();
    for (email, name, roles) in [
        ("ops_lead@example.test", "Ops 100% Lead", json!(["editor"])),
        ("opsxlead@example.test", "Ops 1000 Lead", json!(["author"])),
        (r"back\slash@example.test", r"Back\Slash", json!(["author"])),
        (
            "dormant@example.test",
            "Dormant Editor",
            json!(["author", "editor"]),
        ),
    ] {
        let response = request(
            app,
            Method::POST,
            "/api/v1/admin/users",
            Some(json!({"email":email,"display_name":name,"password":PASSWORD,"roles":roles})),
            Some(&admin_access),
        )
        .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.json);
        // 字段总是存在，从未登录时为 null。
        assert!(response.json["data"].get("last_login_at").is_some());
        assert!(response.json["data"]["last_login_at"].is_null());
        created.push(response.json["data"]["id"].as_str().unwrap().to_owned());
    }
    let dormant_id = &created[3];
    let disabled = request(
        app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{dormant_id}"),
        Some(json!({"status":"disabled"})),
        Some(&admin_access),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::OK, "{}", disabled.json);

    // 详情：登录前 last_login_at 为 null，登录后为时间。
    let lead_path = format!("/api/v1/admin/users/{}", created[0]);
    let before = get(app, &lead_path, &admin_access).await;
    assert_eq!(before.status, StatusCode::OK, "{}", before.json);
    assert_eq!(before.json["data"]["id"], created[0].as_str());
    assert_eq!(before.json["data"]["email"], "ops_lead@example.test");
    assert_eq!(before.json["data"]["roles"], json!(["editor"]));
    assert!(before.json["data"]["last_login_at"].is_null());
    assert!(before.json["data"].get("password_hash").is_none());
    let (lead_access, _, lead_login) = login(app, "ops_lead@example.test").await;
    assert_timestamp(&lead_login["last_login_at"]);
    let after = get(app, &lead_path, &admin_access).await;
    assert_eq!(after.status, StatusCode::OK, "{}", after.json);
    assert_eq!(
        after.json["data"]["last_login_at"],
        lead_login["last_login_at"]
    );
    let missing = get(
        app,
        &format!("/api/v1/admin/users/{}", ids::new_id()),
        &admin_access,
    )
    .await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");
    for bad_id in [
        "550e8400-e29b-41d4-a716-446655440000",
        "019535d9-3df7-79fb-7466-fa907fa17f9e",
        "019535d93df779fbb466fa907fa17f9e",
        "not-an-id",
    ] {
        let response = get(app, &format!("/api/v1/admin/users/{bad_id}"), &admin_access).await;
        assert_field_error(&response, "id", "invalid_format");
    }
    // editor 没有 users.manage，不能读取账户详情或列表。
    for path in [lead_path.as_str(), "/api/v1/admin/users?q=ops"] {
        let forbidden = get(app, path, &lead_access).await;
        assert_error(&forbidden, StatusCode::FORBIDDEN, "forbidden");
    }
    let anonymous = request(app, Method::GET, &lead_path, None, None).await;
    assert_error(&anonymous, StatusCode::UNAUTHORIZED, "unauthorized");

    // 筛选：结果按 created_at DESC、id DESC 排序，total 为筛选后的数量。
    let all_users = 6;
    for (query, expected) in [
        ("", vec![]),
        ("q=%20%20", vec![]),
        ("q=ops_lead", vec!["ops_lead@example.test"]),
        ("q=%20%20OPS_LEAD%20", vec!["ops_lead@example.test"]),
        ("q=_", vec!["ops_lead@example.test"]),
        ("q=100%25", vec!["ops_lead@example.test"]),
        ("q=%25", vec!["ops_lead@example.test"]),
        ("q=%5C", vec![r"back\slash@example.test"]),
        ("q=ops%201000", vec!["opsxlead@example.test"]),
        ("q=dormant%20EDITOR", vec!["dormant@example.test"]),
        ("q=no-such-account", vec![]),
        (
            "role=editor",
            vec!["dormant@example.test", "ops_lead@example.test"],
        ),
        ("role=admin", vec!["admin@example.test"]),
        ("status=disabled", vec!["dormant@example.test"]),
        ("role=editor&status=disabled", vec!["dormant@example.test"]),
        ("role=editor&status=active", vec!["ops_lead@example.test"]),
        (
            "q=ops&role=author&status=active",
            vec!["opsxlead@example.test"],
        ),
        (
            "role=author",
            vec![
                "dormant@example.test",
                r"back\slash@example.test",
                "opsxlead@example.test",
                "author@example.test",
            ],
        ),
        ("role=admin&unknown=ignored", vec!["admin@example.test"]),
    ] {
        let response = get(app, &format!("/api/v1/admin/users?{query}"), &admin_access).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{query}: {}",
            response.json
        );
        let actual = emails(&response);
        let total = &response.json["meta"]["pagination"]["total"];
        if expected.is_empty() && !query.starts_with("q=no") {
            assert_eq!(total, all_users, "{query}");
            assert_eq!(actual.len(), all_users, "{query}");
        } else {
            assert_eq!(actual, expected, "{query}");
            assert_eq!(total, expected.len(), "{query}");
        }
    }
    let paged = get(
        app,
        "/api/v1/admin/users?role=author&per_page=1&page=2",
        &admin_access,
    )
    .await;
    assert_eq!(paged.status, StatusCode::OK, "{}", paged.json);
    assert_eq!(emails(&paged), vec![r"back\slash@example.test"]);
    let pagination = &paged.json["meta"]["pagination"];
    assert_eq!(pagination["total"], 4);
    assert_eq!(pagination["total_pages"], 4);
    let long_query = format!("/api/v1/admin/users?q={}", "a".repeat(101));
    for (query, field, code) in [
        (long_query.as_str(), "q", "out_of_range"),
        ("/api/v1/admin/users?role=owner", "role", "invalid_value"),
        ("/api/v1/admin/users?role=Admin", "role", "invalid_value"),
        ("/api/v1/admin/users?role=", "role", "invalid_value"),
        (
            "/api/v1/admin/users?status=locked",
            "status",
            "invalid_value",
        ),
        ("/api/v1/admin/users?q=a&q=b", "q", "duplicate"),
        (
            "/api/v1/admin/users?status=active&status=disabled",
            "status",
            "duplicate",
        ),
    ] {
        let response = get(app, query, &admin_access).await;
        assert_field_error(&response, field, code);
    }
    // 多个字段同时无效时一次返回全部错误。
    let combined = get(
        app,
        "/api/v1/admin/users?role=x&status=y&per_page=0",
        &admin_access,
    )
    .await;
    for (field, code) in [
        ("role", "invalid_value"),
        ("status", "invalid_value"),
        ("per_page", "out_of_range"),
    ] {
        assert_field_error(&combined, field, code);
    }

    // 角色账户数包含停用账户。
    let roles = get(app, "/api/v1/admin/roles", &admin_access).await;
    assert_eq!(roles.status, StatusCode::OK, "{}", roles.json);
    let counts: std::collections::BTreeMap<_, _> = roles.json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|role| {
            (
                role["key"].as_str().unwrap().to_owned(),
                role["user_count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        counts,
        [("admin", 1), ("author", 4), ("editor", 2)]
            .into_iter()
            .map(|(key, count)| (key.to_owned(), count))
            .collect()
    );
}

/// 以管理员会话 GET 指定路径并断言 200。
async fn admin_get(app: &Router, access: &str, path: &str) -> Response {
    let response = request(app, Method::GET, path, None, Some(access)).await;
    assert_eq!(response.status, StatusCode::OK, "{path}: {}", response.json);
    response
}

fn total(response: &Response) -> i64 {
    response.json["meta"]["pagination"]["total"]
        .as_i64()
        .unwrap()
}

/// 审计记录的请求 ID、操作者摘要、筛选条件与详情接口。
async fn run_audit_logs(app: &Router, pool: &PgPool) {
    const LOGS: &str = "/api/v1/admin/audit-logs";
    let (admin_access, _, admin_session) = login(app, "admin@example.test").await;
    let admin_id = admin_session["id"].as_str().unwrap().to_owned();

    // 登录审计的 request_id 与登录响应头 X-Request-Id 一致，可按 request_id 精确定位。
    let login_request_id: String = sqlx::query_scalar(
        "SELECT request_id::text FROM audit_logs WHERE action = 'auth.login' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let by_login = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?request_id={login_request_id}"),
    )
    .await;
    assert_eq!(total(&by_login), 1, "{}", by_login.json);
    let login_log = &by_login.json["data"][0];
    assert_eq!(login_log["action"], "auth.login");
    assert_eq!(login_log["actor_user_id"], admin_id.as_str());
    assert_eq!(
        login_log["actor"],
        json!({"id": admin_id, "email": "admin@example.test", "display_name": "Acceptance Admin"})
    );

    // 写操作的审计 request_id 等于该写请求的响应头。
    let created = request(
        app,
        Method::POST,
        "/api/v1/admin/users",
        Some(json!({"email":"auditee@example.test","display_name":"Auditee","password":PASSWORD,"roles":["author"]})),
        Some(&admin_access),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    let create_request_id = created.headers["x-request-id"].to_str().unwrap().to_owned();
    let created_id = created.json["data"]["id"].as_str().unwrap().to_owned();
    let by_create = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?request_id={create_request_id}"),
    )
    .await;
    assert_eq!(total(&by_create), 1, "{}", by_create.json);
    let create_log = by_create.json["data"][0].clone();
    assert_eq!(create_log["action"], "users.create");
    assert_eq!(create_log["request_id"], create_request_id.as_str());
    assert_eq!(create_log["resource_id"], created_id.as_str());
    assert_eq!(create_log["actor"]["email"], "admin@example.test");
    let by_resource = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?resource_id={created_id}"),
    )
    .await;
    assert_eq!(total(&by_resource), 1, "{}", by_resource.json);
    assert_eq!(by_resource.json["data"][0]["id"], create_log["id"]);

    // CLI 初始化管理员没有 HTTP 请求与操作账户。
    let bootstrap = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?action=users.bootstrap"),
    )
    .await;
    assert_eq!(total(&bootstrap), 1, "{}", bootstrap.json);
    let bootstrap_log = &bootstrap.json["data"][0];
    assert_eq!(bootstrap_log["request_id"], Value::Null);
    assert_eq!(bootstrap_log["actor"], Value::Null);
    assert_eq!(bootstrap_log["actor_user_id"], Value::Null);
    assert_eq!(bootstrap_log["ip_address"], Value::Null);
    let http_without_request_id: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE request_id IS NULL AND action <> 'users.bootstrap'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(http_without_request_id, 0, "HTTP 审计必须记录 request_id");

    // 前缀匹配：auth.* 覆盖全部 auth. 开头的操作，total 为过滤后数量，分页仍生效。
    let auth_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'auth.%'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(auth_count > 1);
    let auth_page = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?action=auth.*&per_page=1"),
    )
    .await;
    assert_eq!(total(&auth_page), auth_count);
    assert_eq!(auth_page.json["data"].as_array().unwrap().len(), 1);
    let auth_all = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?action=auth.*&per_page=100"),
    )
    .await;
    assert!(
        auth_all.json["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|log| log["action"].as_str().unwrap().starts_with("auth."))
    );
    let exact = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?action=auth&per_page=100"),
    )
    .await;
    assert_eq!(total(&exact), 0, "精确匹配不得按前缀匹配");

    // 前缀中的 `_` 按字面匹配，不作为 LIKE 通配符；同时验证 resource_type 精确筛选。
    for action in ["users_.probe", "usersx.probe"] {
        sqlx::query("INSERT INTO audit_logs (id, action, resource_type) VALUES ($1, $2, 'probe')")
            .bind(ids::new_id())
            .bind(action)
            .execute(pool)
            .await
            .unwrap();
    }
    let escaped = admin_get(app, &admin_access, &format!("{LOGS}?action=users_.*")).await;
    assert_eq!(total(&escaped), 1, "{}", escaped.json);
    assert_eq!(escaped.json["data"][0]["action"], "users_.probe");
    let probes = admin_get(app, &admin_access, &format!("{LOGS}?resource_type=probe")).await;
    assert_eq!(total(&probes), 2, "{}", probes.json);
    let user_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE resource_type = 'user'")
            .fetch_one(pool)
            .await
            .unwrap();
    let users = admin_get(app, &admin_access, &format!("{LOGS}?resource_type=user")).await;
    assert_eq!(total(&users), user_count);

    // 操作者筛选与 AND 组合；结果按 created_at DESC、id DESC 排序。
    let by_actor = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?actor_user_id={admin_id}&per_page=100"),
    )
    .await;
    let actor_logs = by_actor.json["data"].as_array().unwrap();
    assert!(total(&by_actor) >= 2);
    assert!(
        actor_logs
            .iter()
            .all(|log| log["actor_user_id"] == admin_id.as_str())
    );
    let keys: Vec<_> = actor_logs
        .iter()
        .map(|log| {
            (
                chrono::DateTime::parse_from_rfc3339(log["created_at"].as_str().unwrap()).unwrap(),
                log["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(keys.windows(2).all(|pair| pair[0] >= pair[1]), "排序不稳定");
    let combined = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?actor_user_id={admin_id}&action=users.create&resource_type=user"),
    )
    .await;
    assert!(
        combined.json["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|log| log["id"] == create_log["id"])
    );
    let no_match = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?actor_user_id={admin_id}&action=users.bootstrap"),
    )
    .await;
    assert_eq!(total(&no_match), 0);
    assert_eq!(no_match.json["meta"]["pagination"]["total_pages"], 0);

    // 时间范围：from 含、to 不含。
    // API 时间带 `+08:00` 偏移；放入查询串时 `+` 必须编码为 `%2B`，否则会被解码成空格。
    assert_timestamp(&create_log["created_at"]);
    let created_at = create_log["created_at"]
        .as_str()
        .unwrap()
        .replace('+', "%2B");
    let from = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?request_id={create_request_id}&from={created_at}"),
    )
    .await;
    assert_eq!(total(&from), 1, "from 必须包含边界");
    let to = admin_get(
        app,
        &admin_access,
        &format!("{LOGS}?request_id={create_request_id}&to={created_at}"),
    )
    .await;
    assert_eq!(total(&to), 0, "to 不得包含边界");
    let window = admin_get(
        app,
        &admin_access,
        &format!(
            "{LOGS}?request_id={create_request_id}&from=2000-01-01T00:00:00Z&to=2100-01-01T00:00:00%2B08:00"
        ),
    )
    .await;
    assert_eq!(total(&window), 1);

    // 非法筛选值返回字段级错误。
    for (query, field, code) in [
        ("from=2026-13-01T00:00:00Z", "from", "invalid_format"),
        ("to=tomorrow", "to", "invalid_format"),
        (
            "from=2026-11-22T00:00:00Z&to=2026-11-22T00:00:00Z",
            "to",
            "out_of_range",
        ),
        (
            "actor_user_id=67e55044-10b1-426f-9247-bb680e5fe0c8",
            "actor_user_id",
            "invalid_format",
        ),
        (
            "resource_id=019535d9-3df7-79fb-7466-fa907fa17f9e",
            "resource_id",
            "invalid_format",
        ),
        ("request_id=nope", "request_id", "invalid_format"),
        ("action=*", "action", "invalid_value"),
        ("action=auth.%25", "action", "invalid_value"),
        ("resource_type=User", "resource_type", "invalid_value"),
    ] {
        let response = request(
            app,
            Method::GET,
            &format!("{LOGS}?{query}"),
            None,
            Some(&admin_access),
        )
        .await;
        assert_field_error(&response, field, code);
    }
    let long_action = "a".repeat(65);
    let response = request(
        app,
        Method::GET,
        &format!("{LOGS}?action={long_action}"),
        None,
        Some(&admin_access),
    )
    .await;
    assert_field_error(&response, "action", "invalid_value");

    // 详情：200 与列表一致；不存在 404；非法 UUID 422；Author 无权访问。
    let log_id = create_log["id"].as_str().unwrap();
    let detail = admin_get(app, &admin_access, &format!("{LOGS}/{log_id}")).await;
    assert_eq!(detail.json["data"], create_log);
    let missing = request(
        app,
        Method::GET,
        &format!("{LOGS}/{}", ids::new_id()),
        None,
        Some(&admin_access),
    )
    .await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");
    let invalid = request(
        app,
        Method::GET,
        &format!("{LOGS}/67e55044-10b1-426f-9247-bb680e5fe0c8"),
        None,
        Some(&admin_access),
    )
    .await;
    assert_field_error(&invalid, "id", "invalid_format");
    let (author_access, _, _) = login(app, "author@example.test").await;
    for path in [format!("{LOGS}/{log_id}"), format!("{LOGS}?action=auth.*")] {
        let forbidden = request(app, Method::GET, &path, None, Some(&author_access)).await;
        assert_error(&forbidden, StatusCode::FORBIDDEN, "forbidden");
    }
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
        "/api/v1/admin/users/info",
        None,
        None,
    )
    .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);
    let second = request(
        &api_limited,
        Method::GET,
        "/api/v1/admin/users/info",
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
    )
    .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);
    let second = request(
        &login_limited,
        Method::POST,
        "/api/v1/admin/auth/login",
        Some(json!({"email":"missing@example.test","password":PASSWORD})),
        None,
    )
    .await;
    assert_error(&second, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert_eq!(second.headers[header::RETRY_AFTER], "60");
    // 登录限流不得在 API 总额度用尽前阻止其他 API 请求。
    let me = request(
        &login_limited,
        Method::GET,
        "/api/v1/admin/users/info",
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
