#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Authentication required")]
    Unauthorized,
    #[error("Invalid email or password")]
    InvalidCredentials,
    #[error("Permission denied")]
    Forbidden,
    #[error("Resource not found")]
    NotFound,
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("Infrastructure failure: {0}")]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AppError>;
