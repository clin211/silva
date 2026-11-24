//! 站点发布任务接口：查询任务、手动登记全站重建、重试失败任务。

use crate::{
    AppState,
    error::ApiError,
    http::{
        ClientMeta, HttpResult, IdPath, authorize, duplicate_parameter, parse_page, query_pairs,
    },
    response,
};
use axum::{
    extract::{FromRequestParts, State},
    http::{HeaderMap, header, request::Parts},
    response::Response,
};
use silva_domain::site::{PublishJobFilter, PublishJobStatus};
use silva_shared::{
    error::{FieldError, FieldErrorCode},
    pagination::PageRequest,
};

/// 任务列表查询参数：分页加 `status`、`generation`。
pub(crate) struct JobListQuery(PageRequest, PublishJobFilter);
impl<S: Send + Sync> FromRequestParts<S> for JobListQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        let mut filter = PublishJobFilter::default();
        let mut seen = [false; 2];
        for (key, value) in &pairs {
            let index = match key.as_str() {
                "status" => 0,
                "generation" => 1,
                _ => continue,
            };
            if std::mem::replace(&mut seen[index], true) {
                errors.push(duplicate_parameter(key));
                continue;
            }
            if index == 0 {
                match PublishJobStatus::parse(value) {
                    Some(status) => filter.status = Some(status),
                    None => errors.push(FieldError::new(
                        "status",
                        FieldErrorCode::InvalidValue,
                        "status must be one of: queued, building, active, failed, superseded, retired",
                    )),
                }
            } else {
                match value.parse::<i64>() {
                    Ok(generation) if generation > 0 => filter.generation = Some(generation),
                    _ => errors.push(FieldError::new(
                        "generation",
                        FieldErrorCode::InvalidType,
                        "generation must be a positive integer",
                    )),
                }
            }
        }
        if errors.is_empty() {
            Ok(Self(page, filter))
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

pub(crate) async fn list_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    JobListQuery(page, filter): JobListQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state.publishing.jobs(&principal, &page, filter).await?,
    ))
}

pub(crate) async fn get_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state.publishing.job(&principal, id.as_uuid()).await?,
    ))
}

/// 登记全站重建：202，`Location` 指向新任务。
pub(crate) async fn create_job(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let target = state.publishing.request_build(&principal, &meta).await?;
    let mut response = response::accepted(target);
    if let Ok(value) = format!("/api/v1/admin/publishing/jobs/{}", target.publish_job_id).parse() {
        response.headers_mut().insert(header::LOCATION, value);
    }
    Ok(response)
}

/// 重新排队仍为最新目标的失败任务：202，返回任务。
pub(crate) async fn create_attempt(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let job = state
        .publishing
        .retry(&principal, id.as_uuid(), &meta)
        .await?;
    Ok(response::accepted(job))
}
