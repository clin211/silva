//! 统一错误响应：`{"error": {"code", "message", "details"}}`；请求 ID 只通过响应头 `X-Request-Id` 返回。
//! 规范见 `docs/contracts/api-response.md`；错误码在此集中登记，HTTP 状态由错误码唯一决定。

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use silva_application::login_lockout::retry_after_seconds;
use silva_shared::error::{AppError, Conflict, FieldError};

/// 对外错误码，封闭集合。客户端只按 HTTP 状态和此错误码分支，不解析 `message`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MalformedJson,
    Unauthorized,
    TokenExpired,
    InvalidCredentials,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    EmailInUse,
    LastAdminRequired,
    PayloadTooLarge,
    UnsupportedMediaType,
    ValidationError,
    RateLimited,
    InternalError,
}

impl ErrorCode {
    pub const ALL: [Self; 14] = [
        Self::MalformedJson,
        Self::Unauthorized,
        Self::TokenExpired,
        Self::InvalidCredentials,
        Self::Forbidden,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::EmailInUse,
        Self::LastAdminRequired,
        Self::PayloadTooLarge,
        Self::UnsupportedMediaType,
        Self::ValidationError,
        Self::RateLimited,
        Self::InternalError,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::MalformedJson => StatusCode::BAD_REQUEST,
            Self::Unauthorized | Self::TokenExpired | Self::InvalidCredentials => {
                StatusCode::UNAUTHORIZED
            }
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::EmailInUse | Self::LastAdminRequired => StatusCode::CONFLICT,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::ValidationError => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Vec<FieldError>,
    /// 设置时附带 `Retry-After` 响应头（秒）。
    pub retry_after: Option<u64>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
            retry_after: None,
        }
    }

    pub fn validation(details: Vec<FieldError>) -> Self {
        debug_assert!(!details.is_empty(), "校验错误至少包含一条字段错误");
        Self {
            code: ErrorCode::ValidationError,
            message: "Request validation failed".into(),
            details,
            retry_after: None,
        }
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        let code = match error {
            AppError::Validation(details) => return Self::validation(details),
            AppError::Internal(error) => {
                // 内部细节只进日志，响应中不暴露。
                tracing::error!(error = %error, "request failed");
                return Self::new(ErrorCode::InternalError, "Internal server error");
            }
            AppError::LoginLocked { retry_after } => {
                return Self {
                    retry_after: Some(retry_after_seconds(retry_after)),
                    ..Self::new(
                        ErrorCode::RateLimited,
                        "Too many failed login attempts; try again later",
                    )
                };
            }
            AppError::Unauthorized => ErrorCode::Unauthorized,
            AppError::TokenExpired => ErrorCode::TokenExpired,
            AppError::InvalidCredentials => ErrorCode::InvalidCredentials,
            AppError::Forbidden => ErrorCode::Forbidden,
            AppError::NotFound => ErrorCode::NotFound,
            AppError::Conflict(Conflict::EmailInUse) => ErrorCode::EmailInUse,
            AppError::Conflict(Conflict::LastAdminRequired) => ErrorCode::LastAdminRequired,
        };
        Self::new(code, error.to_string())
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorObject<'a>,
}

#[derive(Serialize)]
struct ErrorObject<'a> {
    code: ErrorCode,
    message: &'a str,
    details: &'a [FieldError],
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorObject {
                code: self.code,
                message: &self.message,
                details: &self.details,
            },
        };
        let mut response = (self.code.status(), Json(body)).into_response();
        // RFC 6750：所有 401 声明 Bearer 认证方式；access token 过期时附带 invalid_token。
        if self.code.status() == StatusCode::UNAUTHORIZED {
            let challenge = if self.code == ErrorCode::TokenExpired {
                HeaderValue::from_static(
                    r#"Bearer error="invalid_token", error_description="The access token expired""#,
                )
            } else {
                HeaderValue::from_static("Bearer")
            };
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, challenge);
        }
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}
