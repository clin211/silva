use crate::{
    AppState,
    error::{ApiError, ErrorCode},
    response,
};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use silva_application::{
    identity::UserUpdate,
    ports::{IssuedTokens, RequestMeta, UserFilter},
};
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
use uuid::Uuid;

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

fn duplicate_parameter(key: &str) -> FieldError {
    FieldError::new(
        key.to_owned(),
        FieldErrorCode::Duplicate,
        "parameter is given more than once",
    )
}

/// 从查询参数中解析 `page`、`per_page`，字段错误追加到 `errors`；其他参数忽略。
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
            errors.push(duplicate_parameter(key));
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

/// 账户列表查询参数：分页参数之外，接受可选的 `q`、`role`、`status` 筛选。
/// 筛选参数重复返回 `duplicate`，取值由 `UserFilter::new` 校验；与分页一样忽略其他参数，
/// 并一次返回全部字段错误。
pub(crate) struct UserListInput(PageRequest, UserFilter);
impl<S: Send + Sync> FromRequestParts<S> for UserListInput {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        let mut filters: [Option<String>; 3] = Default::default();
        for (key, value) in pairs {
            let index = match key.as_str() {
                "q" => 0,
                "role" => 1,
                "status" => 2,
                _ => continue,
            };
            if filters[index].is_some() {
                errors.push(duplicate_parameter(&key));
            } else {
                filters[index] = Some(value);
            }
        }
        let [q, role, status] = filters;
        let filter = match UserFilter::new(q, role, status) {
            Ok(filter) => Some(filter),
            Err(AppError::Validation(invalid)) => {
                errors.extend(invalid);
                None
            }
            Err(error) => return Err(error.into()),
        };
        match filter {
            Some(filter) if errors.is_empty() => Ok(Self(page, filter)),
            _ => Err(ApiError::validation(errors)),
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
        let rate_ok = !path.starts_with("/api/")
            || (state
                .limiter
                .allow(ip, false, state.config.api_rate_per_minute)
                && (!PASSWORD_PATHS.contains(&path)
                    || state
                        .limiter
                        .allow(ip, true, state.config.login_rate_per_minute)));
        let mut response = if !rate_ok {
            let mut response =
                ApiError::new(ErrorCode::RateLimited, "Request limit exceeded").into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
            response
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
    response.instrument(span).await
}

/// 从唯一的 `Authorization: Bearer <token>` 请求头取出令牌；缺失、重复、方案不符或格式错误均返回 401。
fn bearer_token(headers: &HeaderMap) -> HttpResult<&str> {
    let unauthorized = || ApiError::from(AppError::Unauthorized);
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(unauthorized());
    };
    let value = value.to_str().map_err(|_| unauthorized())?;
    let (scheme, token) = value.split_once(' ').ok_or_else(unauthorized)?;
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || !token.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(unauthorized());
    }
    Ok(token)
}

/// 校验 access token 并按其会话读取当前账户、角色与权限。
async fn authorize(state: &AppState, headers: &HeaderMap) -> HttpResult<Principal> {
    let token = bearer_token(headers)?;
    Ok(state.identity.authenticate(token).await?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginInput {
    email: String,
    password: String,
}

/// `GET /users/info` 返回的当前账户详情：账户字段加当前权限。
#[derive(Serialize)]
struct UserInfo {
    #[serde(flatten)]
    user: User,
    permissions: Vec<String>,
}

/// 登录与刷新的响应：一对令牌及各自的过期时间（北京时间 RFC 3339，带 `+08:00` 偏移）。
#[derive(Serialize)]
struct TokenPair {
    access_token: String,
    refresh_token: String,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    access_token_expires_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    refresh_token_expires_at: DateTime<Utc>,
}

impl From<IssuedTokens> for TokenPair {
    fn from(tokens: IssuedTokens) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            access_token_expires_at: tokens.access_expires_at,
            refresh_token_expires_at: tokens.refresh_expires_at,
        }
    }
}

pub(crate) async fn login(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    Input(input): Input<LoginInput>,
) -> HttpResult<Response> {
    let tokens = state
        .identity
        .login(input.email, input.password, &meta)
        .await?;
    Ok(response::ok(TokenPair::from(tokens)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshInput {
    refresh_token: String,
}

/// 用 refresh token 换取新的令牌对；无需 Bearer 请求头。
pub(crate) async fn refresh(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    Input(input): Input<RefreshInput>,
) -> HttpResult<Response> {
    let tokens = state.identity.refresh(&input.refresh_token, &meta).await?;
    Ok(response::ok(TokenPair::from(tokens)))
}

/// 当前登录账户的详情、角色与权限；任何有效会话均可读取，不需要额外权限。
pub(crate) async fn user_info(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(UserInfo {
        user: principal.user,
        permissions: principal.permissions,
    }))
}

/// 删除当前会话，使该会话的 access token 与 refresh token 立即失效。
pub(crate) async fn logout(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    state.identity.logout(&principal, &meta).await?;
    Ok(response::no_content())
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
    let principal = authorize(&state, &headers).await?;
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
    UserListInput(page, filter): UserListInput,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state.identity.users(&principal, &page, &filter).await?,
    ))
}

pub(crate) async fn user(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state.identity.user(&principal, id.as_uuid()).await?,
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
    let principal = authorize(&state, &headers).await?;
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
    let principal = authorize(&state, &headers).await?;
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
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(state.identity.roles(&principal).await?))
}

pub(crate) async fn audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: AuditLogQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
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
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state.identity.audit_log(&principal, id.as_uuid()).await?,
    ))
}

// `/health/live` 是给负载均衡与编排系统的存活探针，按惯例返回裸 JSON，不套 `data` 信封。
pub(crate) async fn live() -> Json<Value> {
    Json(json!({"status":"ok"}))
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

    async fn user_list_query(uri: &str) -> HttpResult<UserListInput> {
        let (mut parts, ()) = axum::http::Request::builder()
            .uri(uri)
            .body(())
            .unwrap()
            .into_parts();
        UserListInput::from_request_parts(&mut parts, &()).await
    }

    fn detail_codes(error: ApiError) -> Vec<(String, FieldErrorCode)> {
        assert_eq!(error.code, ErrorCode::ValidationError);
        error
            .details
            .into_iter()
            .map(|detail| (detail.field.into_owned(), detail.code))
            .collect()
    }

    #[tokio::test]
    async fn user_list_query_parses_filters_and_ignores_unknown_parameters() {
        let Ok(UserListInput(page, filter)) =
            user_list_query("/?q=%20Ada%20&role=editor&status=active&page=2&sort=x").await
        else {
            panic!("合法查询应解析成功");
        };
        assert_eq!((page.page, page.per_page), (2, 20));
        assert_eq!(filter.q.as_deref(), Some("Ada"));
        assert_eq!(filter.role.as_deref(), Some("editor"));
        assert_eq!(filter.status.as_deref(), Some("active"));
        let Ok(UserListInput(_, blank)) = user_list_query("/?q=%20%20").await else {
            panic!("空白关键字视为未提供");
        };
        assert_eq!(blank, UserFilter::default());
    }

    #[tokio::test]
    async fn user_list_query_rejects_duplicates_and_invalid_values() {
        for (uri, field, code) in [
            ("/?role=owner", "role", FieldErrorCode::InvalidValue),
            ("/?status=locked", "status", FieldErrorCode::InvalidValue),
            ("/?q=a&q=b", "q", FieldErrorCode::Duplicate),
            (
                "/?role=admin&role=author",
                "role",
                FieldErrorCode::Duplicate,
            ),
            (
                "/?per_page=0&role=x",
                "per_page",
                FieldErrorCode::OutOfRange,
            ),
            ("/?per_page=0&role=x", "role", FieldErrorCode::InvalidValue),
        ] {
            let Err(error) = user_list_query(uri).await else {
                panic!("{uri} 应校验失败");
            };
            assert!(
                detail_codes(error).contains(&(field.to_owned(), code)),
                "{uri}: 缺少 {field}/{code:?}"
            );
        }
    }

    #[test]
    fn bearer_token_requires_single_well_formed_authorization_header() {
        let headers = |values: &[&str]| {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
            }
            headers
        };
        assert_eq!(
            bearer_token(&headers(&["Bearer abc.def.ghi"])).unwrap(),
            "abc.def.ghi"
        );
        assert_eq!(bearer_token(&headers(&["bearer abc"])).unwrap(), "abc");
        for invalid in [
            &[][..],
            &["Bearer"],
            &["Bearer "],
            &["Bearer a b"],
            &["Bearer  abc"],
            &["Basic YWRtaW46c2VjcmV0"],
            &["abc.def.ghi"],
            &["Bearer abc", "Bearer abc"],
        ] {
            let error = bearer_token(&headers(invalid)).unwrap_err();
            assert_eq!(error.code, ErrorCode::Unauthorized, "{invalid:?}");
        }
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
