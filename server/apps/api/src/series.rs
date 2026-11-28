//! 后台系列、目录成员与公开目标接口。
use crate::{
    AppState,
    content::{ChildPath, UnpublishQuery, required_version},
    error::ApiError,
    http::{
        ClientMeta, HttpResult, IdPath, Input, authorize, duplicate_parameter, parse_page,
        parse_uuid, query_pairs,
    },
    response,
};
use axum::{
    extract::{FromRequestParts, State},
    http::{HeaderMap, request::Parts},
    response::Response,
};
use serde::Deserialize;
use silva_application::series::{Actor, Edit, Filter, Sort};
use silva_domain::series::{CompletionStatus, ItemInput, Metadata, MetadataPatch, Outline};
use silva_shared::{
    error::{AppError, FieldError, FieldErrorCode},
    ids::EntityId,
    pagination::PageRequest,
};

pub(crate) struct List(PageRequest, Filter);
impl<S: Send + Sync> FromRequestParts<S> for List {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        let mut filter = Filter::default();
        let mut seen = std::collections::HashSet::new();
        for (key, value) in &pairs {
            if !matches!(
                key.as_str(),
                "q" | "category_id" | "published" | "completion_status" | "sort"
            ) {
                continue;
            }
            if !seen.insert(key) {
                errors.push(duplicate_parameter(key));
                continue;
            }
            let invalid = || {
                FieldError::new(
                    key.clone(),
                    FieldErrorCode::InvalidValue,
                    "unsupported filter value",
                )
            };
            let result = match key.as_str() {
                "q" if value.contains('\0') => Err(invalid()),
                "q" => {
                    filter.q = Some(value.trim().to_owned());
                    Ok(())
                }
                "category_id" => {
                    parse_uuid("category_id", value).map(|id| filter.category_id = Some(id))
                }
                "published" => value
                    .parse::<bool>()
                    .map(|b| filter.published = Some(b))
                    .map_err(|_| invalid()),
                "completion_status" => match value.as_str() {
                    "ongoing" => Ok(CompletionStatus::Ongoing),
                    "completed" => Ok(CompletionStatus::Completed),
                    _ => Err(invalid()),
                }
                .map(|status| filter.completion_status = Some(status)),
                _ => match value.as_str() {
                    "updated_desc" => Ok(Sort::UpdatedDesc),
                    "created_desc" => Ok(Sort::CreatedDesc),
                    "sort_order_asc" => Ok(Sort::SortOrderAsc),
                    _ => Err(invalid()),
                }
                .map(|sort| filter.sort = sort),
            };
            if let Err(e) = result {
                errors.push(e);
            }
        }
        if !errors.is_empty() {
            return Err(ApiError::validation(errors));
        }
        Ok(Self(page, filter))
    }
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    List(page, filter): List,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state.series.list(&principal, &page, &filter).await?,
    ))
}
pub(crate) async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state.series.detail(&principal, id.as_uuid()).await?,
    ))
}
pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    Input(input): Input<Metadata>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let data = state
        .series
        .create(
            input,
            Actor {
                principal: &principal,
                meta: &meta,
            },
        )
        .await?;
    Ok(response::created_at(
        &format!("/api/v1/admin/series/{}", data.summary.id),
        data,
    ))
}
pub(crate) async fn patch(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    IdPath(id): IdPath,
    Input(input): Input<MetadataPatch>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .series
            .edit(
                id.as_uuid(),
                Edit::Metadata(input),
                Actor {
                    principal: &principal,
                    meta: &meta,
                },
            )
            .await?,
    ))
}
pub(crate) async fn outline(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    IdPath(id): IdPath,
    Input(input): Input<Outline>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .series
            .edit(
                id.as_uuid(),
                Edit::Outline(input),
                Actor {
                    principal: &principal,
                    meta: &meta,
                },
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Append {
    expected_edit_version: i64,
    section_id: Option<EntityId>,
    post_id: EntityId,
    display_title: Option<String>,
}
pub(crate) async fn append(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    IdPath(id): IdPath,
    Input(input): Input<Append>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let post = input.post_id;
    let edit = Edit::Append {
        version: input.expected_edit_version,
        section: input.section_id.map(EntityId::as_uuid),
        item: ItemInput {
            post_id: post,
            display_title: input.display_title,
        },
    };
    let data = state
        .series
        .edit(
            id.as_uuid(),
            edit,
            Actor {
                principal: &principal,
                meta: &meta,
            },
        )
        .await?;
    let item = data
        .series
        .items()
        .find(|i| i.post_id == post.as_uuid())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("appended item is missing")))?;
    Ok(response::created_at(
        &format!("/api/v1/admin/series/{id}/items/{post}"),
        serde_json::json!({"item":item,"edit_version":data.series.summary.edit_version,"publication":data.publication}),
    ))
}

pub(crate) struct EditVersion(i64);
impl<S: Send + Sync> FromRequestParts<S> for EditVersion {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        required_version(&pairs, "expected_edit_version").map(Self)
    }
}
pub(crate) struct MemberPath(ChildPath);
impl<S: Send + Sync> FromRequestParts<S> for MemberPath {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        ChildPath::parse(parts, state, "post_id").await.map(Self)
    }
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    MemberPath(path): MemberPath,
    EditVersion(version): EditVersion,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    state
        .series
        .edit(
            path.post,
            Edit::Remove {
                version,
                post: path.child,
            },
            Actor {
                principal: &principal,
                meta: &meta,
            },
        )
        .await?;
    Ok(response::no_content())
}
pub(crate) async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    IdPath(id): IdPath,
    EditVersion(edit): EditVersion,
    UnpublishQuery(publication): UnpublishQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    state
        .series
        .delete(
            id.as_uuid(),
            edit,
            publication,
            Actor {
                principal: &principal,
                meta: &meta,
            },
        )
        .await?;
    Ok(response::no_content())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Publication {
    expected_publication_version: i64,
}

pub(crate) async fn publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    IdPath(id): IdPath,
    Input(input): Input<Publication>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .series
            .publish(
                id.as_uuid(),
                input.expected_publication_version,
                true,
                Actor {
                    principal: &principal,
                    meta: &meta,
                },
            )
            .await?,
    ))
}
pub(crate) async fn unpublish(
    State(state): State<AppState>,
    headers: HeaderMap,
    ClientMeta(meta): ClientMeta,
    IdPath(id): IdPath,
    UnpublishQuery(version): UnpublishQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .series
            .publish(
                id.as_uuid(),
                version,
                false,
                Actor {
                    principal: &principal,
                    meta: &meta,
                },
            )
            .await?,
    ))
}
