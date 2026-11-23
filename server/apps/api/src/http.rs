use crate::{
    AppState,
    error::{ApiError, ErrorCode, REQUEST_ID},
    response,
};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use silva_application::{identity::UserUpdate, ports::RequestMeta};
use silva_domain::identity::{
    AuditLogFilter, Principal, User, parse_audit_action, parse_audit_resource_type,
};
use silva_infrastructure::persistence;
use silva_shared::{
    error::{AppError, FieldError, FieldErrorCode},
    ids::{self, EntityId},
    pagination::PageRequest,
};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;

const SESSION_COOKIE: &str = "__Host-silva_session";
/// 需要校验密码的入口共用登录额度，防止借修改密码接口暴力猜测当前密码。
const PASSWORD_PATHS: [&str; 2] = ["/api/v1/admin/auth/login", "/api/v1/admin/auth/password"];
type HttpResult<T> = Result<T, ApiError>;

/// JSON 请求体提取器：语法错误返回 400 `malformed_json`；类型、缺失或未知字段返回
/// 422 `validation_error`，并在 `details` 中给出字段路径。
pub(crate) struct Input<T>(T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Input<T> {
    type Rejection = ApiError;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        if !is_json_content_type(req.headers()) {
            return Err(ApiError::new(
                ErrorCode::UnsupportedMediaType,
                "Expected application/json",
            ));
        }
        let bytes = Bytes::from_request(req, state).await.map_err(|rejection| {
            if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::new(ErrorCode::PayloadTooLarge, "Request body exceeds 64 KiB")
            } else {
                ApiError::new(ErrorCode::MalformedJson, "Request body could not be read")
            }
        })?;
        let malformed =
            || ApiError::new(ErrorCode::MalformedJson, "Request body is not valid JSON");
        let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
        let value = serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
            if error.inner().classify() == serde_json::error::Category::Data {
                ApiError::validation(vec![json_field_error(
                    &error.path().to_string(),
                    &error.inner().to_string(),
                )])
            } else {
                malformed()
            }
        })?;
        deserializer.end().map_err(|_| malformed())?;
        Ok(Self(value))
    }
}

/// 接受 `application/json` 与 `application/*+json`，参数（如 charset）忽略。
fn is_json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let essence = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    essence == "application/json"
        || essence
            .strip_prefix("application/")
            .is_some_and(|subtype| subtype.ends_with("+json"))
}

/// 将 serde 的数据错误转换为字段错误。只使用路径和字段名，不回显输入值，避免泄露密码等敏感内容。
fn json_field_error(path: &str, message: &str) -> FieldError {
    let parent = (path != "." && path != "?").then_some(path);
    // 缺失字段报在父级路径上；未知字段的路径已包含字段名本身。
    let join = |name: &str| match parent {
        Some(parent) if parent == name || parent.ends_with(&format!(".{name}")) => {
            parent.to_owned()
        }
        Some(parent) => format!("{parent}.{name}"),
        None => name.to_owned(),
    };
    let named = |prefix: &str| {
        message
            .strip_prefix(prefix)
            .and_then(|rest| rest.split('`').next())
    };
    if let Some(name) = named("missing field `") {
        FieldError::new(join(name), FieldErrorCode::Required, "field is required")
    } else if let Some(name) = named("unknown field `") {
        FieldError::new(
            join(name),
            FieldErrorCode::UnknownField,
            "field is not allowed",
        )
    } else {
        FieldError::new(
            parent.unwrap_or("").to_owned(),
            FieldErrorCode::InvalidType,
            "value has an invalid type",
        )
    }
}

/// 读取查询字符串的全部键值对；编码无效时返回 422 `invalid_format`。
async fn query_pairs<S: Send + Sync>(
    parts: &mut Parts,
    state: &S,
) -> HttpResult<Vec<(String, String)>> {
    let Query(pairs) = Query::<Vec<(String, String)>>::from_request_parts(parts, state)
        .await
        .map_err(|_| {
            ApiError::validation(vec![FieldError::new(
                "",
                FieldErrorCode::InvalidFormat,
                "query string is not valid URL encoding",
            )])
        })?;
    Ok(pairs)
}

/// 解析 `page`、`per_page`：十进制整数，重复或越界写入字段错误；其他参数忽略。
fn parse_page(pairs: &[(String, String)], errors: &mut Vec<FieldError>) -> PageRequest {
    let mut page = PageRequest::default();
    let mut seen = [false; 2];
    for (key, value) in pairs {
        let (slot, index) = match key.as_str() {
            "page" => (&mut page.page, 0),
            "per_page" => (&mut page.per_page, 1),
            _ => continue,
        };
        if std::mem::replace(&mut seen[index], true) {
            errors.push(FieldError::new(
                key.clone(),
                FieldErrorCode::Duplicate,
                "parameter is given more than once",
            ));
            continue;
        }
        match value.parse::<u32>() {
            Ok(parsed) => *slot = parsed,
            Err(_) if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                errors.push(FieldError::new(
                    key.clone(),
                    FieldErrorCode::OutOfRange,
                    "value is too large",
                ));
            }
            Err(_) => errors.push(FieldError::new(
                key.clone(),
                FieldErrorCode::InvalidType,
                "value must be a non-negative integer",
            )),
        }
    }
    if let Err(AppError::Validation(range)) = page.validate() {
        errors.extend(range);
    }
    page
}

/// 分页查询参数：`page`、`per_page` 为十进制整数，重复或越界均返回字段错误；其他参数忽略。
pub(crate) struct PageInput(PageRequest);
impl<S: Send + Sync> FromRequestParts<S> for PageInput {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        if errors.is_empty() {
            Ok(Self(page))
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

/// 审计列表查询参数：分页参数加可选筛选条件；同一参数重复或取值无效均返回字段错误，
/// 不回显输入值。未定义的参数忽略。
pub(crate) struct AuditLogQuery {
    page: PageRequest,
    filter: AuditLogFilter,
}
impl<S: Send + Sync> FromRequestParts<S> for AuditLogQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        const FILTERS: [&str; 7] = [
            "from",
            "to",
            "actor_user_id",
            "resource_id",
            "request_id",
            "action",
            "resource_type",
        ];
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        let mut filter = AuditLogFilter::default();
        let mut seen = HashSet::new();
        for (key, value) in &pairs {
            let Some(&name) = FILTERS.iter().find(|name| **name == key) else {
                continue;
            };
            if !seen.insert(name) {
                errors.push(FieldError::new(
                    name,
                    FieldErrorCode::Duplicate,
                    "parameter is given more than once",
                ));
                continue;
            }
            let result = match name {
                "from" => parse_time(name, value).map(|time| filter.from = Some(time)),
                "to" => parse_time(name, value).map(|time| filter.to = Some(time)),
                "actor_user_id" => {
                    parse_uuid(name, value).map(|id| filter.actor_user_id = Some(id))
                }
                "resource_id" => parse_uuid(name, value).map(|id| filter.resource_id = Some(id)),
                "request_id" => parse_uuid(name, value).map(|id| filter.request_id = Some(id)),
                "action" => parse_audit_action(value).map(|action| filter.action = Some(action)),
                _ => parse_audit_resource_type(value)
                    .map(|resource_type| filter.resource_type = Some(resource_type)),
            };
            if let Err(error) = result {
                errors.push(error);
            }
        }
        if let Err(AppError::Validation(range)) = filter.validate() {
            errors.extend(range);
        }
        if errors.is_empty() {
            Ok(Self { page, filter })
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

/// 解析 RFC 3339 时间并统一换算为 UTC。
fn parse_time(field: &'static str, value: &str) -> Result<DateTime<Utc>, FieldError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| {
            FieldError::new(
                field,
                FieldErrorCode::InvalidFormat,
                "value must be an RFC 3339 timestamp",
            )
        })
}

/// 解析带连字符的 UUIDv7，格式、版本位或变体位不符时返回 `invalid_format`。
fn parse_uuid(field: &'static str, value: &str) -> Result<Uuid, FieldError> {
    value
        .parse::<EntityId>()
        .map(EntityId::as_uuid)
        .map_err(|_| {
            FieldError::new(
                field,
                FieldErrorCode::InvalidFormat,
                "value must be a hyphenated UUIDv7",
            )
        })
}

/// 当前请求的服务端 UUIDv7，由 `context` 中间件写入请求扩展，与响应头 `X-Request-Id` 一致。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestId(pub Uuid);

/// 审计用的请求来源：直连 peer IP、User-Agent 与请求 ID；不信任客户端提供的转发头。
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
        let request_id = parts.extensions.get::<RequestId>().map(|id| id.0);
        Ok(Self(RequestMeta {
            ip_address,
            user_agent,
            request_id,
        }))
    }
}

pub(crate) struct IdPath(EntityId);
impl<S: Send + Sync> FromRequestParts<S> for IdPath {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let invalid = || {
            ApiError::validation(vec![FieldError::new(
                "id",
                FieldErrorCode::InvalidFormat,
                "id must be a hyphenated UUIDv7",
            )])
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
    mut req: Request,
    next: Next,
) -> HttpResult<Response> {
    use tracing::Instrument;
    let request_id = ids::new_id();
    // 处理器通过请求扩展读取同一个请求 ID，写入审计记录。
    req.extensions_mut().insert(RequestId(request_id));
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
            let mut response =
                ApiError::new(ErrorCode::RateLimited, "Request limit exceeded").into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
            response
        } else if !origin_ok {
            ApiError::new(
                ErrorCode::OriginRejected,
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
                ErrorCode::CsrfRejected,
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
/// 登录与读取会话共用的会话视图，作为 `data` 返回。
#[derive(Serialize)]
struct AuthSession {
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
    let mut response = response::ok(AuthSession {
        user: session.principal.user,
        permissions: session.principal.permissions,
        csrf_token: session.csrf_token,
    });
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
    Ok(response::ok(AuthSession {
        user: principal.user,
        permissions: principal.permissions,
        csrf_token,
    }))
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, true).await?;
    state.identity.logout(&principal, &meta).await?;
    let mut response = response::no_content();
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
    Ok(response::no_content())
}

pub(crate) async fn users(
    State(state): State<AppState>,
    headers: HeaderMap,
    PageInput(page): PageInput,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(response::page(
        state.identity.users(&principal, &page).await?,
    ))
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
    let user = state
        .identity
        .create_user(
            &principal,
            input.email,
            input.display_name,
            input.password,
            input.roles,
            &meta,
        )
        .await?;
    Ok(response::created(user))
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
    let user = state
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
        .await?;
    Ok(response::ok(user))
}

pub(crate) async fn roles(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(response::ok(state.identity.roles(&principal).await?))
}

pub(crate) async fn audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: AuditLogQuery,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(response::page(
        state
            .identity
            .audit_logs(&principal, &query.page, &query.filter)
            .await?,
    ))
}

pub(crate) async fn audit_log(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    Ok(response::ok(
        state.identity.audit_log(&principal, id.as_uuid()).await?,
    ))
}

pub(crate) async fn system_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let (principal, _) = authorize(&state, &headers, false).await?;
    principal.require("system.read")?;
    Ok(response::ok(dependency_health(&state).await?))
}

// `/health/*` 是给负载均衡与编排系统的探针，按惯例返回裸 JSON，不套 `data` 信封。
pub(crate) async fn live() -> Json<Value> {
    Json(json!({"status":"ok"}))
}
pub(crate) async fn ready(State(state): State<AppState>) -> HttpResult<Json<Value>> {
    Ok(Json(dependency_health(&state).await?))
}

async fn dependency_health(state: &AppState) -> HttpResult<Value> {
    persistence::ready(&state.pool).await.map_err(|error| {
        tracing::warn!(%error, "readiness check failed");
        ApiError::unavailable()
    })?;
    Ok(json!({"status":"ok", "database":"up"}))
}

pub(crate) async fn openapi() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        include_str!("../openapi.json"),
    )
}
pub(crate) async fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "Resource not found")
}
pub(crate) async fn method_not_allowed() -> ApiError {
    ApiError::new(ErrorCode::MethodNotAllowed, "Method not allowed")
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
    fn json_content_type_accepts_json_and_structured_suffix() {
        let headers = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
            headers
        };
        for accepted in [
            "application/json",
            "Application/JSON; charset=utf-8",
            "application/merge-patch+json",
        ] {
            assert!(is_json_content_type(&headers(accepted)), "{accepted}");
        }
        for rejected in ["text/json", "application/jsonp", "text/plain"] {
            assert!(!is_json_content_type(&headers(rejected)), "{rejected}");
        }
        assert!(!is_json_content_type(&HeaderMap::new()));
    }

    #[test]
    fn json_field_error_uses_path_without_echoing_input() {
        let missing = json_field_error(".", "missing field `email` at line 1 column 2");
        assert_eq!(
            (missing.field.as_ref(), missing.code),
            ("email", FieldErrorCode::Required)
        );
        let nested = json_field_error("profile", "unknown field `x`, expected `a`");
        assert_eq!(
            (nested.field.as_ref(), nested.code),
            ("profile.x", FieldErrorCode::UnknownField)
        );
        let typed = json_field_error(
            "roles[1]",
            "invalid type: string \"secret\", expected u32 at line 1 column 9",
        );
        assert_eq!(
            (typed.field.as_ref(), typed.code),
            ("roles[1]", FieldErrorCode::InvalidType)
        );
        assert!(!typed.message.contains("secret"));
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
