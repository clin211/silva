//! 媒体管理接口：申请直传、列表、详情、确认上传与替代文本、授权读取内容和删除。

use crate::{
    AppState,
    error::ApiError,
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
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use silva_application::{
    media::{MediaUpdate, UploadTicket},
    ports::MediaFilter,
};
use silva_domain::media::{Media, MediaStatus, UploadRequest};
use silva_shared::{
    error::{FieldError, FieldErrorCode},
    pagination::PageRequest,
};
use std::collections::BTreeMap;

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
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMediaInput {
    filename: String,
    declared_mime_type: String,
    size_bytes: u64,
}

/// 浏览器直传说明：按 `method` 向 `url` 发送文件原始字节，并原样携带 `headers`。
#[derive(Serialize)]
struct UploadInstruction {
    method: String,
    url: String,
    headers: BTreeMap<String, String>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct MediaUpload {
    media: Media,
    upload: UploadInstruction,
}

impl From<UploadTicket> for MediaUpload {
    fn from(ticket: UploadTicket) -> Self {
        Self {
            upload: UploadInstruction {
                method: ticket.upload.method,
                url: ticket.upload.url,
                headers: ticket.upload.headers.into_iter().collect(),
                expires_at: ticket.media.upload_expires_at,
            },
            media: ticket.media,
        }
    }
}

pub(crate) async fn create(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    Input(input): Input<CreateMediaInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let ticket = state
        .media
        .create_upload(
            &principal,
            UploadRequest {
                filename: input.filename,
                declared_mime_type: input.declared_mime_type,
                size_bytes: input.size_bytes,
            },
            &meta,
        )
        .await?;
    let location = format!("/api/v1/admin/media/{}", ticket.media.id);
    Ok(response::created_at(&location, MediaUpload::from(ticket)))
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
    Ok(response::ok(
        state.media.get(&principal, id.as_uuid()).await?,
    ))
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
    state.media.delete(&principal, id.as_uuid(), &meta).await?;
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
