//! 跨层共享的业务错误。HTTP 状态码与对外错误码的映射只在 API 层完成，
//! 规范见 `docs/contracts/api-response.md`。

use std::{borrow::Cow, fmt};

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
    /// 输入不满足约束；至少包含一条字段级错误。
    #[error("Request validation failed: {}", FieldErrors(.0))]
    Validation(Vec<FieldError>),
    #[error("{}", .0.message())]
    Conflict(Conflict),
    #[error("Infrastructure failure: {0}")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// 构造只含一条字段错误的校验失败。
    pub fn invalid(
        field: impl Into<Cow<'static, str>>,
        code: FieldErrorCode,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::Validation(vec![FieldError::new(field, code, message)])
    }
}

/// 与当前资源状态冲突的业务原因；每个变体对应一个对外错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// 邮箱已被其他账户占用。
    EmailInUse,
    /// 操作后系统将不再有启用状态的管理员。
    LastAdminRequired,
}

impl Conflict {
    pub fn message(self) -> &'static str {
        match self {
            Self::EmailInUse => "Email is already in use",
            Self::LastAdminRequired => "At least one active administrator is required",
        }
    }
}

/// 字段级错误，对外序列化为错误响应 `error.details` 的元素。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FieldError {
    /// 出错位置：请求体字段用点号与下标表示（如 `roles[1]`），查询和路径参数使用参数名。
    pub field: Cow<'static, str>,
    pub code: FieldErrorCode,
    pub message: Cow<'static, str>,
}

impl FieldError {
    pub fn new(
        field: impl Into<Cow<'static, str>>,
        code: FieldErrorCode,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            field: field.into(),
            code,
            message: message.into(),
        }
    }
}

/// 字段级错误码，封闭集合；新增取值须同步更新 OpenAPI 与响应规范文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldErrorCode {
    /// 缺少必填字段。
    Required,
    /// 出现未定义的字段。
    UnknownField,
    /// JSON 类型不符，如需要字符串却收到数字。
    InvalidType,
    /// 格式不符，如邮箱或 UUIDv7。
    InvalidFormat,
    /// 长度、数量或数值超出范围。
    OutOfRange,
    /// 不在允许的取值集合内。
    InvalidValue,
    /// 集合中出现重复元素。
    Duplicate,
    /// 与服务端保存的值不符，如当前密码错误。
    Incorrect,
    /// 新值必须与原值不同。
    Unchanged,
}

impl FieldErrorCode {
    pub const ALL: [Self; 9] = [
        Self::Required,
        Self::UnknownField,
        Self::InvalidType,
        Self::InvalidFormat,
        Self::OutOfRange,
        Self::InvalidValue,
        Self::Duplicate,
        Self::Incorrect,
        Self::Unchanged,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::UnknownField => "unknown_field",
            Self::InvalidType => "invalid_type",
            Self::InvalidFormat => "invalid_format",
            Self::OutOfRange => "out_of_range",
            Self::InvalidValue => "invalid_value",
            Self::Duplicate => "duplicate",
            Self::Incorrect => "incorrect",
            Self::Unchanged => "unchanged",
        }
    }
}

struct FieldErrors<'a>(&'a [FieldError]);
impl fmt::Display for FieldErrors<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{}: {}", error.field, error.message)?;
        }
        Ok(())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_display_lists_every_field() {
        let error = AppError::Validation(vec![
            FieldError::new("email", FieldErrorCode::InvalidFormat, "bad email"),
            FieldError::new("roles", FieldErrorCode::OutOfRange, "empty"),
        ]);
        assert_eq!(
            error.to_string(),
            "Request validation failed: email: bad email; roles: empty"
        );
    }
}
