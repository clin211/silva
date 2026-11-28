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
    UploadIncomplete,
    UploadExpired,
    MediaNotReady,
    MediaInUse,
    SlugInUse,
    SlugLocked,
    RevisionConflict,
    PublicationConflict,
    AlreadyUnpublished,
    SeriesEditConflict,
    SeriesInUse,
    PostInUse,
    PostAlreadyInSeries,
    AlreadyPublished,
    TaxonomyInUse,
    SiteGenerationChanged,
    PayloadTooLarge,
    UnsupportedMediaType,
    ValidationError,
    MediaInvalid,
    RateLimited,
    InternalError,
    StorageUnavailable,
}

impl ErrorCode {
    pub const ALL: [Self; 32] = [
        Self::MalformedJson,
        Self::Unauthorized,
        Self::TokenExpired,
        Self::InvalidCredentials,
        Self::Forbidden,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::EmailInUse,
        Self::LastAdminRequired,
        Self::UploadIncomplete,
        Self::UploadExpired,
        Self::MediaNotReady,
        Self::MediaInUse,
        Self::SlugInUse,
        Self::SlugLocked,
        Self::RevisionConflict,
        Self::PublicationConflict,
        Self::AlreadyUnpublished,
        Self::SeriesEditConflict,
        Self::SeriesInUse,
        Self::PostInUse,
        Self::PostAlreadyInSeries,
        Self::AlreadyPublished,
        Self::TaxonomyInUse,
        Self::SiteGenerationChanged,
        Self::PayloadTooLarge,
        Self::UnsupportedMediaType,
        Self::ValidationError,
        Self::MediaInvalid,
        Self::RateLimited,
        Self::InternalError,
        Self::StorageUnavailable,
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
            Self::EmailInUse
            | Self::LastAdminRequired
            | Self::UploadIncomplete
            | Self::UploadExpired
            | Self::MediaNotReady
            | Self::MediaInUse
            | Self::SlugInUse
            | Self::SlugLocked
            | Self::RevisionConflict
            | Self::PublicationConflict
            | Self::AlreadyUnpublished
            | Self::SeriesEditConflict
            | Self::PostInUse
            | Self::SeriesInUse
            | Self::PostAlreadyInSeries
            | Self::AlreadyPublished
            | Self::TaxonomyInUse
            | Self::SiteGenerationChanged => StatusCode::CONFLICT,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::ValidationError | Self::MediaInvalid => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE,
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
    pub media_usage: Option<Box<silva_application::ports::MediaUsage>>,
    /// 设置时附带 `X-Site-Generation` 响应头，告知客户端当前线上版本。
    pub generation: Option<i64>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
            retry_after: None,
            media_usage: None,
            generation: None,
        }
    }

    pub fn validation(details: Vec<FieldError>) -> Self {
        debug_assert!(!details.is_empty(), "校验错误至少包含一条字段错误");
        Self {
            code: ErrorCode::ValidationError,
            message: "Request validation failed".into(),
            details,
            retry_after: None,
            media_usage: None,
            generation: None,
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
            AppError::Conflict(Conflict::UploadIncomplete) => ErrorCode::UploadIncomplete,
            AppError::Conflict(Conflict::UploadExpired) => ErrorCode::UploadExpired,
            AppError::Conflict(Conflict::MediaNotReady) => ErrorCode::MediaNotReady,
            AppError::Conflict(Conflict::MediaInUse) => ErrorCode::MediaInUse,
            AppError::MediaInvalid { .. } => ErrorCode::MediaInvalid,
            AppError::StorageUnavailable => ErrorCode::StorageUnavailable,
            AppError::Conflict(Conflict::SlugInUse) => ErrorCode::SlugInUse,
            AppError::Conflict(Conflict::SlugLocked) => ErrorCode::SlugLocked,
            AppError::Conflict(Conflict::RevisionConflict) => ErrorCode::RevisionConflict,
            AppError::Conflict(Conflict::PublicationConflict) => ErrorCode::PublicationConflict,
            AppError::Conflict(Conflict::AlreadyUnpublished) => ErrorCode::AlreadyUnpublished,
            AppError::Conflict(Conflict::SeriesEditConflict) => ErrorCode::SeriesEditConflict,
            AppError::Conflict(Conflict::PostInUse) => ErrorCode::PostInUse,
            AppError::Conflict(Conflict::SeriesInUse) => ErrorCode::SeriesInUse,
            AppError::Conflict(Conflict::PostAlreadyInSeries) => ErrorCode::PostAlreadyInSeries,
            AppError::Conflict(Conflict::AlreadyPublished) => ErrorCode::AlreadyPublished,
            AppError::Conflict(Conflict::TaxonomyInUse) => ErrorCode::TaxonomyInUse,
            AppError::Conflict(Conflict::SiteGenerationChanged) => ErrorCode::SiteGenerationChanged,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    media_usage: Option<&'a silva_application::ports::MediaUsage>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorObject {
                code: self.code,
                message: &self.message,
                details: &self.details,
                media_usage: self.media_usage.as_deref(),
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
        if let Some(generation) = self.generation {
            response
                .headers_mut()
                .insert("x-site-generation", HeaderValue::from(generation));
        }
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}
