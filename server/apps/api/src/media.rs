//! 媒体管理接口：按内容去重上传、列表、详情、旧版上传确认与替代文本、授权读取内容和删除。

use crate::{
    AppState,
    error::{ApiError, ErrorCode},
    http::{
        ClientMeta, HttpResult, IdPath, Input, authorize, duplicate_parameter, parse_page,
        query_pairs,
    },
    response,
};
use axum::{
    extract::{FromRequestParts, State},
    http::{HeaderMap, HeaderValue, header, request::Parts},
    response::{IntoResponse, Response},
};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use silva_application::{media::MediaUpdate, ports::MediaFilter};
use silva_domain::media::{ALLOWED_MIME_TYPES, MAX_UPLOAD_BYTES};
use silva_domain::media::{Media, MediaStatus, UploadRequest};
use silva_shared::{
    error::{FieldError, FieldErrorCode},
    pagination::PageRequest,
};

/// 媒体列表查询参数：分页参数加可选的 `status` 筛选；重复或无效取值返回字段错误。
pub(crate) struct MediaListQuery(PageRequest, MediaFilter);
impl<S: Send + Sync> FromRequestParts<S> for MediaListQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        let mut filter = MediaFilter::default();
        let mut seen = false;
        for (key, value) in &pairs {
            if key != "status" {
                continue;
            }
            if std::mem::replace(&mut seen, true) {
                errors.push(duplicate_parameter(key));
                continue;
            }
            match MediaStatus::parse(value) {
                Some(status) => filter.status = Some(status),
                None => errors.push(FieldError::new(
                    "status",
                    FieldErrorCode::InvalidValue,
                    "status must be one of: pending, ready, rejected",
                )),
            }
        }
        if errors.is_empty() {
            Ok(Self(page, filter))
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

#[derive(Deserialize)]
struct UploadQuery {
    filename: Option<String>,
}

#[derive(Serialize)]
struct MediaUpload {
    media: Media,
    url: String,
}

pub(crate) async fn create(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    request: axum::extract::Request,
) -> HttpResult<Response> {
    let headers = request.headers();
    let principal = authorize(&state, headers).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    if content_type != "application/octet-stream" && !ALLOWED_MIME_TYPES.contains(&content_type) {
        return Err(ApiError::new(
            ErrorCode::UnsupportedMediaType,
            "Expected raw JPEG, PNG or WebP image bytes",
        ));
    }
    let size_bytes = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            ApiError::validation(vec![FieldError::new(
                "Content-Length",
                FieldErrorCode::Required,
                "File length is required",
            )])
        })?;
    if size_bytes > MAX_UPLOAD_BYTES {
        return Err(ApiError::new(
            ErrorCode::PayloadTooLarge,
            "File exceeds 10 MiB",
        ));
    }
    crate::http::validate_query_encoding(request.uri())?;
    let query = axum::extract::Query::<UploadQuery>::try_from_uri(request.uri()).map_err(|_| {
        ApiError::validation(vec![FieldError::new(
            "filename",
            FieldErrorCode::InvalidFormat,
            "Invalid filename",
        )])
    })?;
    let upload = UploadRequest {
        filename: query
            .0
            .filename
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "image".to_owned()),
        declared_mime_type: content_type.to_owned(),
        size_bytes,
    };
    let body = request
        .into_body()
        .into_data_stream()
        .map_err(std::io::Error::other);
    let (media, url, created) = state
        .media
        .upload(&principal, upload, Box::pin(body), &meta)
        .await?;
    let location = format!("/api/v1/admin/media/{}", media.id);
    let result = MediaUpload { media, url };
    Ok(if created {
        response::created_at(&location, result)
    } else {
        response::ok(result)
    })
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    MediaListQuery(page, filter): MediaListQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state.media.list(&principal, &page, filter).await?,
    ))
}

pub(crate) async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let (media, usage) = state.media.detail(&principal, id.as_uuid()).await?;
    Ok(response::with_meta(media, usage))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMediaInput {
    status: Option<String>,
    alt_text: Option<String>,
}

pub(crate) async fn update(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    Input(input): Input<UpdateMediaInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let media = state
        .media
        .update(
            &principal,
            id.as_uuid(),
            MediaUpdate {
                status: input.status,
                alt_text: input.alt_text,
            },
            &meta,
        )
        .await?;
    Ok(response::ok(media))
}

/// 以流式响应返回已校验文件的原始字节。内容类型取服务端识别的格式；沙箱 CSP 防止被当作页面执行。
pub(crate) async fn content(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let content = state.media.content(&principal, id.as_uuid()).await?;
    let content_type = content
        .media
        .mime_type
        .as_deref()
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    // 按块转发存储中的内容，不在 API 进程内缓冲整个文件。
    let mut response = axum::body::Body::from_stream(content.body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(content.size));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    Ok(response)
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    if let Some(usage) = state.media.delete(&principal, id.as_uuid(), &meta).await? {
        let mut error = ApiError::from(silva_shared::error::AppError::Conflict(
            silva_shared::error::Conflict::MediaInUse,
        ));
        error.media_usage = Some(Box::new(usage));
        return Err(error);
    }
    Ok(response::no_content())
}

#[cfg(test)]
mod tests {
    use super::*;
    use silva_shared::error::FieldErrorCode;

    async fn query(uri: &str) -> HttpResult<MediaListQuery> {
        let (mut parts, ()) = axum::http::Request::builder()
            .uri(uri)
            .body(())
            .unwrap()
            .into_parts();
        MediaListQuery::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn list_query_parses_status_and_rejects_duplicates_or_unknown_values() {
        let Ok(MediaListQuery(page, filter)) = query("/?status=ready&per_page=5&x=1").await else {
            panic!("合法查询应解析成功");
        };
        assert_eq!(page.per_page, 5);
        assert_eq!(filter.status, Some(MediaStatus::Ready));
        assert_eq!(filter.uploaded_by, None, "上传者不由查询参数决定");
        for (uri, code) in [
            ("/?status=uploaded", FieldErrorCode::InvalidValue),
            ("/?status=ready&status=pending", FieldErrorCode::Duplicate),
        ] {
            let Err(error) = query(uri).await else {
                panic!("{uri} 应校验失败");
            };
            assert!(
                error
                    .details
                    .iter()
                    .any(|detail| detail.field == "status" && detail.code == code),
                "{uri}"
            );
        }
    }
}
