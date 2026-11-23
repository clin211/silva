use crate::{
    AppState,
    error::{ApiError, REQUEST_ID},
};
use axum::{
    Json,
    extract::{
        ConnectInfo, FromRequest, FromRequestParts, Path, Query, Request, State,
        rejection::JsonRejection,
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use silva_application::{identity::UserUpdate, ports::RequestMeta};
use silva_domain::identity::{Principal, User};
use silva_infrastructure::persistence;
use silva_shared::{
    error::AppError,
    ids::{self, EntityId},
    pagination::PageRequest,
};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

const SESSION_COOKIE: &str = "__Host-silva_session";
/// 需要校验密码的入口共用登录额度，防止借修改密码接口暴力猜测当前密码。
const PASSWORD_PATHS: [&str; 2] = ["/api/v1/admin/auth/login", "/api/v1/admin/auth/password"];
type HttpResult<T> = Result<T, ApiError>;

pub(crate) struct Input<T>(T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Input<T> {
    type Rejection = ApiError;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection: JsonRejection| {
                let status = rejection.status();
                if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
                    ApiError::new(
                        status,
                        "unsupported_media_type",
                        "Expected application/json",
                    )
                } else if status == StatusCode::PAYLOAD_TOO_LARGE {
                    ApiError::new(status, "payload_too_large", "Request body exceeds 64 KiB")
                } else {
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "validation_error",
                        "Invalid JSON request",
                    )
                }
            })
    }
}

pub(crate) struct PageInput(PageRequest);
impl<S: Send + Sync> FromRequestParts<S> for PageInput {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let Query(page) = Query::<PageRequest>::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "validation_error",
                    "Invalid pagination",
                )
            })?;
        page.validate().map_err(ApiError::from)?;
        Ok(Self(page))
    }
}

/// 审计用的请求来源：直连 peer IP 与 User-Agent；不信任客户端提供的转发头。
pub(crate) struct ClientMeta(RequestMeta);
impl<S: Send + Sync> FromRequestParts<S> for ClientMeta {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ip_address = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|value| value.0.ip().to_canonical());
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Ok(Self(RequestMeta {
            ip_address,
            user_agent,
        }))
    }
}

pub(crate) struct IdPath(EntityId);
impl<S: Send + Sync> FromRequestParts<S> for IdPath {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let invalid = || {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_uuid_v7",
                "Expected a hyphenated UUIDv7 entity ID",
            )
        };
        let Path(value) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|_| invalid())?;
        value.parse().map(Self).map_err(|_| invalid())
    }
}

/// 限流表最多跟踪的客户端数量，防止大量来源地址耗尽内存。
const RATE_LIMIT_MAX_KEYS: usize = 10_000;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(crate) struct RateLimiter {
    state: Mutex<RateLimiterState>,
}

#[derive(Default)]
struct RateLimiterState {
    buckets: HashMap<(IpAddr, bool), (Instant, u32)>,
    last_sweep: Option<Instant>,
}

impl RateLimiter {
    fn allow(&self, ip: IpAddr, login: bool, limit: u32) -> bool {
        // 计数器在任何中断点都保持一致，锁中毒时继续使用内部数据，避免全站永久 429。
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let key = (rate_limit_key(ip), login);
        // 过期窗口按需清理：表满或距上次清理超过一个窗口时才全表扫描，避免每个请求 O(n)。
        let sweep_due = state
            .last_sweep
            .is_none_or(|last| now.duration_since(last) >= RATE_LIMIT_WINDOW);
        if sweep_due || state.buckets.len() >= RATE_LIMIT_MAX_KEYS {
            state
                .buckets
                .retain(|_, (start, _)| now.duration_since(*start) < RATE_LIMIT_WINDOW);
            state.last_sweep = Some(now);
        }
        if state.buckets.len() >= RATE_LIMIT_MAX_KEYS && !state.buckets.contains_key(&key) {
            return false;
        }
        let (start, count) = state.buckets.entry(key).or_insert((now, 0));
        if now.duration_since(*start) >= RATE_LIMIT_WINDOW {
            *start = now;
            *count = 0;
        }
        if *count >= limit {
            return false;
        }
        *count += 1;
        true
    }
}

/// IPv6 客户端通常拥有整个 /64，按前缀合并计数，避免单个客户端轮换地址绕过限流或占满限流表。
fn rate_limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let mut segments = v6.segments();
                segments[4..].fill(0);
                IpAddr::V6(Ipv6Addr::from(segments))
            }
        },
    }
}

pub(crate) async fn context(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> HttpResult<Response> {
    use tracing::Instrument;
    let request_id = ids::new_id();
    let method = req.method().clone();
    // 仅记录路由模板，避免路径或查询参数中的用户输入和凭据进入日志。
    let route = req
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".into());
    let span = tracing::info_span!("http.request", otel.name = %format!("{method} {route}"), otel.kind = "server",
        %request_id, %method, %route, trace_id = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty, otel.status_code = tracing::field::Empty);
    let headers = ["traceparent", "tracestate"]
        .into_iter()
        .filter_map(|name| {
            req.headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| (name.to_owned(), value.to_owned()))
        })
        .collect();
    silva_shared::telemetry::set_parent(&span, &headers);
    let trace_id = silva_shared::telemetry::trace_id(&span);
    span.record("trace_id", &trace_id);
    let response = async move {
        let _in_flight = state.metrics.http_started();
        let started = Instant::now();
        let ip = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|value| value.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let path = req.uri().path();
        let admin_write = path.starts_with("/api/v1/admin/")
            && ![Method::GET, Method::HEAD, Method::OPTIONS].contains(&method);
        let rate_ok = !path.starts_with("/api/")
            || (state
                .limiter
                .allow(ip, false, state.config.api_rate_per_minute)
                && (!PASSWORD_PATHS.contains(&path)
                    || state
                        .limiter
                        .allow(ip, true, state.config.login_rate_per_minute)));
        let origin_ok =
            !admin_write || exact_header(req.headers(), "origin", &state.config.admin_origin);
        let mut response = if !rate_ok {
            let mut response = ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Request limit exceeded",
            )
            .into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
            response
        } else if !origin_ok {
            ApiError::new(
                StatusCode::FORBIDDEN,
                "origin_rejected",
                "Origin must match the configured admin origin",
            )
            .into_response()
        } else {
            next.run(req).await
        };
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&request_id.to_string())
                .map_err(|error| AppError::Internal(error.into()))?,
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response.headers_mut().insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
        let elapsed = started.elapsed();
        state.metrics.record_http(
            method.as_str(),
            &route,
            response.status().as_u16(),
            elapsed.as_secs_f64(),
        );
        let current = tracing::Span::current();
        current.record("http.response.status_code", response.status().as_u16());
        if response.status().is_server_error() {
            current.record("otel.status_code", "ERROR");
        }
        response.headers_mut().insert(
            "x-trace-id",
            HeaderValue::from_str(&trace_id).map_err(|error| AppError::Internal(error.into()))?,
        );
        tracing::info!(%request_id, %method, %route, status = response.status().as_u16(), latency_ms = elapsed.as_secs_f64() * 1000.0, "http request");
        Ok(response)
    };
    REQUEST_ID
        .scope(request_id, response.instrument(span))
        .await
}

fn exact_header(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    let mut values = headers.get_all(name).iter();
    values.next().and_then(|v| v.to_str().ok()) == Some(expected) && values.next().is_none()
}

fn session_token(headers: &HeaderMap) -> HttpResult<String> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value
            .to_str()
            .map_err(|_| ApiError::from(AppError::Unauthorized))?;
        for pair in value.split(';') {
            if let Some((name, token)) = pair.trim().split_once('=')
                && name == SESSION_COOKIE
            {
                if found.is_some() {
                    return Err(AppError::Unauthorized.into());
                }
                found = Some(token.to_owned());
            }
        }
    }
    found.ok_or_else(|| AppError::Unauthorized.into())
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    write: bool,
) -> HttpResult<(Principal, String)> {
    let token = session_token(headers)?;
    let principal = state.identity.authenticate(&token).await?;
    let csrf = state.identity.csrf_token(&token);
    if write {
        let candidate = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !bool::from(candidate.as_bytes().ct_eq(csrf.as_bytes()))
            || headers.get_all("x-csrf-token").iter().count() != 1
        {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "csrf_rejected",
                "CSRF token does not match session",
            ));
        }
    }
    Ok((principal, csrf))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginInput {
    email: String,
    password: String,
}
#[derive(Serialize)]
struct AuthResponse {
    user: User,
    permissions: Vec<String>,
    csrf_token: String,
}

pub(crate) async fn login(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    Input(input): Input<LoginInput>,
) -> HttpResult<Response> {
    let session = state
        .identity
        .login(input.email, input.password, &meta)
        .await?;
    let cookie = format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={}",
        session.token,
        state.config.session_ttl.as_secs()
    );
    let mut response = Json(AuthResponse {
        user: session.principal.user,
        permissions: session.principal.permissions,
        csrf_token: session.csrf_token,
    })
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| {
            ApiError::from(AppError::Internal(anyhow::anyhow!(
                "invalid session cookie"
            )))
        })?,
    );
    Ok(response)
}

pub(crate) async fn me(State(state): State<AppState>, headers: HeaderMap) -> HttpResult<Response> {
    let (principal, csrf_token) = authorize(&state, &headers, false).await?;
    Ok(Json(AuthResponse {
        user: principal.user,
        permissions: principal.permissions,
        csrf_token,
    })
    .into_response())
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, true).await?;
    state.identity.logout(&principal, &meta).await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "__Host-silva_session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0",
        ),
    );
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangePasswordInput {
    current_password: String,
    new_password: String,
}
pub(crate) async fn change_password(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    Input(input): Input<ChangePasswordInput>,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, true).await?;
    state
        .identity
        .change_password(
            &principal,
            input.current_password,
            input.new_password,
            &meta,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn users(
    State(state): State<AppState>,
    headers: HeaderMap,
    PageInput(page): PageInput,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(Json(state.identity.users(&principal, &page).await?).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateUserInput {
    email: String,
    display_name: String,
    password: String,
    roles: Vec<String>,
}
pub(crate) async fn create_user(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    Input(input): Input<CreateUserInput>,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, true).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .identity
                .create_user(
                    &principal,
                    input.email,
                    input.display_name,
                    input.password,
                    input.roles,
                    &meta,
                )
                .await?,
        ),
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateUserInput {
    display_name: Option<String>,
    status: Option<String>,
    roles: Option<Vec<String>>,
    password: Option<String>,
}
pub(crate) async fn update_user(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    Input(input): Input<UpdateUserInput>,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, true).await?;
    Ok(Json(
        state
            .identity
            .update_user(
                &principal,
                id.as_uuid(),
                UserUpdate {
                    display_name: input.display_name,
                    status: input.status,
                    roles: input.roles,
                    password: input.password,
                },
                &meta,
            )
            .await?,
    )
    .into_response())
}

pub(crate) async fn roles(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(Json(state.identity.roles(&principal).await?).into_response())
}

pub(crate) async fn audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    PageInput(page): PageInput,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(Json(state.identity.audit_logs(&principal, &page).await?).into_response())
}

pub(crate) async fn system_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Json<Value>> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    principal.require("system.read")?;
    ready(State(state)).await
}

pub(crate) async fn live() -> Json<Value> {
    Json(json!({"status":"ok"}))
}
pub(crate) async fn ready(State(state): State<AppState>) -> HttpResult<Json<Value>> {
    persistence::ready(&state.pool).await.map_err(|error| {
        tracing::warn!(%error, "readiness check failed");
        ApiError::unavailable()
    })?;
    Ok(Json(json!({"status":"ok", "database":"up"})))
}

pub(crate) async fn openapi() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        include_str!("../openapi.json"),
    )
}
pub(crate) async fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "Resource not found")
}
pub(crate) async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "Method not allowed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_groups_ipv6_by_64_prefix() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::2".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(rate_limit_key(a), rate_limit_key(b));
        assert_ne!(rate_limit_key(a), rate_limit_key(other));
        let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
        assert_eq!(
            rate_limit_key(mapped),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn rate_limit_rejects_after_limit_and_separates_login_bucket() {
        let limiter = RateLimiter::default();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.allow(ip, false, 2));
        assert!(limiter.allow(ip, false, 2));
        assert!(!limiter.allow(ip, false, 2));
        assert!(limiter.allow(ip, true, 1));
        assert!(!limiter.allow(ip, true, 1));
        // IPv6 同一 /64 共享额度。
        let v6a: IpAddr = "2001:db8::1".parse().unwrap();
        let v6b: IpAddr = "2001:db8::2".parse().unwrap();
        assert!(limiter.allow(v6a, false, 1));
        assert!(!limiter.allow(v6b, false, 1));
    }

    #[test]
    fn rate_limit_survives_poisoned_lock() {
        let limiter = std::sync::Arc::new(RateLimiter::default());
        let poisoner = limiter.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.state.lock().unwrap();
            panic!("poison the limiter lock");
        })
        .join();
        assert!(limiter.state.is_poisoned());
        assert!(limiter.allow("192.0.2.9".parse().unwrap(), false, 1));
    }
}
