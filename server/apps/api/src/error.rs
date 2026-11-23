use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use silva_shared::error::AppError;
use uuid::Uuid;

tokio::task_local! { pub static REQUEST_ID: Uuid; }

pub(crate) struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    pub fn unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "Service dependency unavailable",
        )
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        let (status, code) = match &error {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
            AppError::Conflict(code) => (StatusCode::CONFLICT, *code),
            AppError::Internal(error) => {
                tracing::error!(error = %error, "request failed");
                return Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Internal server error",
                );
            }
        };
        Self::new(status, code, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let id = REQUEST_ID
            .try_with(|id| *id)
            .unwrap_or_else(|_| silva_shared::ids::new_id());
        (self.status, Json(json!({"error": {"code": self.code, "message": self.message, "details": [], "request_id": id}}))).into_response()
    }
}
