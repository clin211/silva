//! 统一错误响应：`{"error": {"code", "message", "details", "request_id"}}`。
//! 规范见 `docs/contracts/api-response.md`；错误码在此集中登记，HTTP 状态由错误码唯一决定。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use silva_shared::error::{AppError, Conflict, FieldError};
use uuid::Uuid;

tokio::task_local! { pub static REQUEST_ID: Uuid; }

/// 对外错误码，封闭集合。客户端只按 HTTP 状态和此错误码分支，不解析 `message`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MalformedJson,
    Unauthorized,
    InvalidCredentials,
    Forbidden,
    OriginRejected,
    CsrfRejected,
    NotFound,
    MethodNotAllowed,
    EmailInUse,
    LastAdminRequired,
    PayloadTooLarge,
    UnsupportedMediaType,
    ValidationError,
    RateLimited,
    InternalError,
    ServiceUnavailable,
}

impl ErrorCode {
    pub const ALL: [Self; 16] = [
        Self::MalformedJson,
        Self::Unauthorized,
        Self::InvalidCredentials,
        Self::Forbidden,
        Self::OriginRejected,
        Self::CsrfRejected,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::EmailInUse,
        Self::LastAdminRequired,
        Self::PayloadTooLarge,
        Self::UnsupportedMediaType,
        Self::ValidationError,
        Self::RateLimited,
        Self::InternalError,
        Self::ServiceUnavailable,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::MalformedJson => StatusCode::BAD_REQUEST,
            Self::Unauthorized | Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::OriginRejected | Self::CsrfRejected => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::EmailInUse | Self::LastAdminRequired => StatusCode::CONFLICT,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::ValidationError => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Vec<FieldError>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
        }
    }

    pub fn validation(details: Vec<FieldError>) -> Self {
        debug_assert!(!details.is_empty(), "校验错误至少包含一条字段错误");
        Self {
            code: ErrorCode::ValidationError,
            message: "Request validation failed".into(),
            details,
        }
    }

    pub fn unavailable() -> Self {
        Self::new(
            ErrorCode::ServiceUnavailable,
            "Service dependency unavailable",
        )
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
            AppError::Unauthorized => ErrorCode::Unauthorized,
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
    request_id: Uuid,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = REQUEST_ID
            .try_with(|id| *id)
            .unwrap_or_else(|_| silva_shared::ids::new_id());
        let body = ErrorBody {
            error: ErrorObject {
                code: self.code,
                message: &self.message,
                details: &self.details,
                request_id,
            },
        };
        (self.code.status(), Json(body)).into_response()
    }
}
