//! 媒体领域规则：允许的图片类型、大小与像素上限、状态取值和对象键。
//!
//! 一期只接收 JPEG、PNG、WebP 图片。服务端按实际字节识别格式、流式写入私有对象，完整解码并提交后记录变为 `ready`。

use chrono::{DateTime, Utc};
use serde::Serialize;
use silva_shared::error::{AppError, FieldError, FieldErrorCode, Result};
use uuid::Uuid;

/// 单个媒体文件的最大字节数（10 MiB）。
pub const MAX_UPLOAD_BYTES: u64 = 10 * 1024 * 1024;
/// 解码后允许的最大像素数，限制解码内存（RGBA 约 160 MB）。
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;
/// 单边最大像素数。
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;
/// 允许的 MIME 类型；最终类型以服务端识别的实际格式为准。
pub const ALLOWED_MIME_TYPES: [&str; 3] = ["image/jpeg", "image/png", "image/webp"];
/// 原文件名去首尾空白后的最大字节数。
pub const FILENAME_MAX_BYTES: usize = 255;
/// 替代文本去首尾空白后的最大字节数。
pub const ALT_TEXT_MAX_BYTES: usize = 1000;

/// 媒体状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaStatus {
    /// 上传尚未完成或待校验。
    Pending,
    /// 已校验并写入正式对象，可被引用。
    Ready,
    /// 未通过校验，需重新申请上传。
    Rejected,
}

impl MediaStatus {
    pub const ALL: [Self; 3] = [Self::Pending, Self::Ready, Self::Rejected];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

/// 媒体被拒绝的原因，保存在记录中并在 `media_invalid` 错误中返回。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    /// 实际大小超过 10 MiB。
    TooLarge,
    /// 实际字节不是 JPEG、PNG 或 WebP。
    UnsupportedFormat,
    /// 文件头可识别但无法完整解码。
    Undecodable,
    /// 宽、高或总像素数超过上限。
    DimensionsExceeded,
}

impl RejectionReason {
    pub const ALL: [Self; 4] = [
        Self::TooLarge,
        Self::UnsupportedFormat,
        Self::Undecodable,
        Self::DimensionsExceeded,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::UnsupportedFormat => "unsupported_format",
            Self::Undecodable => "undecodable",
            Self::DimensionsExceeded => "dimensions_exceeded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }

    /// 转为对外的 `422 media_invalid` 错误。
    pub fn into_error(self) -> AppError {
        AppError::MediaInvalid {
            reason: self.as_str(),
        }
    }
}

/// 媒体记录的对外表示；不包含对象存储键等内部信息。
#[derive(Clone, Debug, Serialize)]
pub struct Media {
    pub id: Uuid,
    pub status: MediaStatus,
    pub original_filename: String,
    /// 请求声明的图片类型；通用二进制请求使用服务端识别的类型。
    pub declared_mime_type: String,
    /// 申请上传时客户端声明的字节数。
    pub declared_size_bytes: i64,
    /// 服务端识别的实际 MIME 类型；未通过校验时为 None。
    pub mime_type: Option<String>,
    /// 服务端读取的实际字节数；未通过校验时为 None。
    pub size_bytes: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub alt_text: Option<String>,
    /// 仅 `rejected` 状态有值。
    pub rejection_reason: Option<RejectionReason>,
    /// 上传者账户 ID；账户关联缺失时为 None。
    pub uploaded_by: Option<Uuid>,
    /// 未完成上传的清理截止时间。
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub upload_expires_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

/// 申请上传时提交的声明信息，经 [`UploadRequest::validate`] 规范化。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadRequest {
    pub filename: String,
    pub declared_mime_type: String,
    pub size_bytes: u64,
}

impl UploadRequest {
    /// 去除文件名首尾空白并校验全部字段，一次返回所有字段错误。
    pub fn validate(self) -> Result<Self> {
        let mut errors = Vec::new();
        let filename = self.filename.trim().to_owned();
        if filename.is_empty() || filename.len() > FILENAME_MAX_BYTES {
            errors.push(FieldError::new(
                "filename",
                FieldErrorCode::OutOfRange,
                "filename must contain 1 to 255 bytes",
            ));
        } else if filename.chars().any(char::is_control) {
            errors.push(FieldError::new(
                "filename",
                FieldErrorCode::InvalidFormat,
                "filename must not contain control characters",
            ));
        }
        if self.declared_mime_type != "application/octet-stream"
            && !ALLOWED_MIME_TYPES.contains(&self.declared_mime_type.as_str())
        {
            errors.push(FieldError::new(
                "declared_mime_type",
                FieldErrorCode::InvalidValue,
                "declared_mime_type must be one of: image/jpeg, image/png, image/webp, application/octet-stream",
            ));
        }
        if !(1..=MAX_UPLOAD_BYTES).contains(&self.size_bytes) {
            errors.push(FieldError::new(
                "size_bytes",
                FieldErrorCode::OutOfRange,
                "size_bytes must be between 1 and 10485760",
            ));
        }
        if errors.is_empty() {
            Ok(Self { filename, ..self })
        } else {
            Err(AppError::Validation(errors))
        }
    }
}

/// 规范化替代文本：去首尾空白，空白视为清除（None）；超长或含控制字符返回字段错误。
pub fn normalize_alt_text(value: &str) -> Result<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > ALT_TEXT_MAX_BYTES {
        return Err(AppError::invalid(
            "alt_text",
            FieldErrorCode::OutOfRange,
            "alt_text must contain at most 1000 bytes",
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(AppError::invalid(
            "alt_text",
            FieldErrorCode::InvalidFormat,
            "alt_text must not contain control characters",
        ));
    }
    Ok(Some(value.to_owned()))
}

/// 浏览器直传使用的私有临时对象键。
pub fn upload_key(id: Uuid) -> String {
    format!("uploads/{id}")
}

/// 校验通过后的不可覆盖正式对象键。
pub fn storage_key(id: Uuid) -> String {
    format!("media/{id}")
}

/// 校验通过的图片信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedImage {
    pub mime_type: &'static str,
    pub size_bytes: u64,
    pub width: u32,
    pub height: u32,
}

/// 按尺寸上限判定已识别的图片；不负责解码。
pub fn check_dimensions(width: u32, height: u32) -> std::result::Result<(), RejectionReason> {
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        Err(RejectionReason::DimensionsExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(filename: &str, mime: &str, size: u64) -> UploadRequest {
        UploadRequest {
            filename: filename.into(),
            declared_mime_type: mime.into(),
            size_bytes: size,
        }
    }

    fn codes(error: AppError) -> Vec<(String, FieldErrorCode)> {
        match error {
            AppError::Validation(errors) => errors
                .into_iter()
                .map(|error| (error.field.into_owned(), error.code))
                .collect(),
            other => panic!("期望校验错误，实际为 {other:?}"),
        }
    }

    #[test]
    fn upload_request_trims_filename_and_accepts_limits() {
        let valid = request("  cover.png ", "image/png", MAX_UPLOAD_BYTES)
            .validate()
            .unwrap();
        assert_eq!(valid.filename, "cover.png");
        assert!(request("a.webp", "image/webp", 1).validate().is_ok());
        assert!(
            request(&"长".repeat(85), "image/jpeg", 10)
                .validate()
                .is_ok(),
            "255 字节的多字节文件名合法"
        );
    }

    #[test]
    fn upload_request_reports_every_invalid_field() {
        let error = request(" ", "image/svg+xml", MAX_UPLOAD_BYTES + 1)
            .validate()
            .unwrap_err();
        assert_eq!(
            codes(error),
            vec![
                ("filename".into(), FieldErrorCode::OutOfRange),
                ("declared_mime_type".into(), FieldErrorCode::InvalidValue),
                ("size_bytes".into(), FieldErrorCode::OutOfRange),
            ]
        );
        let error = request("a\nb.png", "image/png", 0).validate().unwrap_err();
        assert_eq!(
            codes(error),
            vec![
                ("filename".into(), FieldErrorCode::InvalidFormat),
                ("size_bytes".into(), FieldErrorCode::OutOfRange),
            ]
        );
        assert!(
            request(&"长".repeat(86), "image/png", 1)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn alt_text_blank_clears_and_limits_bytes() {
        assert_eq!(normalize_alt_text("  ").unwrap(), None);
        assert_eq!(
            normalize_alt_text(" 封面 ").unwrap().as_deref(),
            Some("封面")
        );
        assert!(normalize_alt_text(&"a".repeat(ALT_TEXT_MAX_BYTES)).is_ok());
        assert!(normalize_alt_text(&"a".repeat(ALT_TEXT_MAX_BYTES + 1)).is_err());
        assert!(normalize_alt_text("a\u{0}b").is_err());
    }

    #[test]
    fn dimensions_limit_sides_and_total_pixels() {
        assert!(check_dimensions(1, 1).is_ok());
        assert!(check_dimensions(MAX_IMAGE_DIMENSION, 2000).is_ok());
        assert!(check_dimensions(0, 10).is_err());
        assert!(check_dimensions(MAX_IMAGE_DIMENSION + 1, 1).is_err());
        assert_eq!(
            check_dimensions(10_000, 5_000),
            Err(RejectionReason::DimensionsExceeded)
        );
    }

    #[test]
    fn status_and_reason_round_trip() {
        for status in MediaStatus::ALL {
            assert_eq!(MediaStatus::parse(status.as_str()), Some(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::json!(status.as_str())
            );
        }
        for reason in RejectionReason::ALL {
            assert_eq!(RejectionReason::parse(reason.as_str()), Some(reason));
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                serde_json::json!(reason.as_str())
            );
        }
        assert_eq!(MediaStatus::parse("uploaded"), None);
    }

    #[test]
    fn object_keys_separate_temporary_and_formal_prefixes() {
        let id = silva_shared::ids::new_id();
        assert_eq!(upload_key(id), format!("uploads/{id}"));
        assert_eq!(storage_key(id), format!("media/{id}"));
    }
}
