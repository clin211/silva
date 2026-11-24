//! 文章管理接口：文章、修订、审核与发布。

use crate::{
    AppState,
    error::ApiError,
    http::{
        ClientMeta, HttpResult, IdPath, Input, authorize, duplicate_parameter, parse_page,
        parse_uuid, query_pairs,
    },
    response,
};
use axum::{
    extract::{FromRequestParts, Path, State},
    http::{HeaderMap, request::Parts},
    response::Response,
};
use serde::Deserialize;
use silva_application::content::RevisionResult;
use silva_domain::content::{PostFilter, ReviewStatus, RevisionContent};
use silva_shared::{
    error::{FieldError, FieldErrorCode},
    ids::EntityId,
    pagination::PageRequest,
};
use uuid::Uuid;

/// 修订内容字段，创建文章与保存修订的请求体共用，转换为领域输入。
struct RevisionFields {
    title: String,
    excerpt: Option<String>,
    markdown_body: String,
    category_ids: Vec<EntityId>,
    tag_ids: Vec<EntityId>,
    cover_media_id: Option<EntityId>,
    is_featured: bool,
    seo_title: Option<String>,
    seo_description: Option<String>,
}

impl From<RevisionFields> for RevisionContent {
    fn from(fields: RevisionFields) -> Self {
        let ids = |ids: Vec<EntityId>| ids.into_iter().map(EntityId::as_uuid).collect();
        Self {
            title: fields.title,
            excerpt: fields.excerpt,
            markdown_body: fields.markdown_body,
            category_ids: ids(fields.category_ids),
            tag_ids: ids(fields.tag_ids),
            cover_media_id: fields.cover_media_id.map(EntityId::as_uuid),
            is_featured: fields.is_featured,
            seo_title: fields.seo_title,
            seo_description: fields.seo_description,
        }
    }
}

pub(crate) async fn create_post(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    Input(input): Input<CreatePostBody>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let (slug, content) = input.into_parts();
    let post = state
        .content
        .create_post(&principal, slug, content, &meta)
        .await?;
    let location = format!("/api/v1/admin/posts/{}", post.id);
    Ok(response::created_at(&location, post))
}

/// 创建文章的请求体：slug 加完整修订内容。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreatePostBody {
    slug: String,
    title: String,
    #[serde(default)]
    excerpt: Option<String>,
    markdown_body: String,
    #[serde(default)]
    category_ids: Vec<EntityId>,
    #[serde(default)]
    tag_ids: Vec<EntityId>,
    #[serde(default)]
    cover_media_id: Option<EntityId>,
    #[serde(default)]
    is_featured: bool,
    #[serde(default)]
    seo_title: Option<String>,
    #[serde(default)]
    seo_description: Option<String>,
}

impl CreatePostBody {
    fn into_parts(self) -> (String, RevisionContent) {
        (
            self.slug,
            RevisionFields {
                title: self.title,
                excerpt: self.excerpt,
                markdown_body: self.markdown_body,
                category_ids: self.category_ids,
                tag_ids: self.tag_ids,
                cover_media_id: self.cover_media_id,
                is_featured: self.is_featured,
                seo_title: self.seo_title,
                seo_description: self.seo_description,
            }
            .into(),
        )
    }
}

/// 文章列表查询参数：分页加 `q`、`author_id`、`category_id`、`tag_id`、`review_status`、`published`。
pub(crate) struct PostListQuery(PageRequest, PostFilter);
impl<S: Send + Sync> FromRequestParts<S> for PostListQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        const NAMES: [&str; 6] = [
            "q",
            "author_id",
            "category_id",
            "tag_id",
            "review_status",
            "published",
        ];
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        let mut filter = PostFilter::default();
        let mut seen = std::collections::HashSet::new();
        for (key, value) in &pairs {
            let Some(&name) = NAMES.iter().find(|name| **name == key) else {
                continue;
            };
            if !seen.insert(name) {
                errors.push(duplicate_parameter(key));
                continue;
            }
            let result = match name {
                "q" => {
                    let q = value.trim();
                    if q.len() > 100 {
                        Err(FieldError::new(
                            "q",
                            FieldErrorCode::OutOfRange,
                            "q must contain at most 100 bytes",
                        ))
                    } else {
                        filter.q = (!q.is_empty()).then(|| q.to_owned());
                        Ok(())
                    }
                }
                "author_id" => parse_uuid(name, value).map(|id| filter.author_id = Some(id)),
                "category_id" => parse_uuid(name, value).map(|id| filter.category_id = Some(id)),
                "tag_id" => parse_uuid(name, value).map(|id| filter.tag_id = Some(id)),
                "review_status" => ReviewStatus::parse(value)
                    .map(|status| filter.review_status = Some(status))
                    .ok_or_else(|| {
                        FieldError::new(
                            name,
                            FieldErrorCode::InvalidValue,
                            "review_status must be one of: pending, approved, rejected",
                        )
                    }),
                _ => match value.as_str() {
                    "true" | "false" => {
                        filter.published = Some(value == "true");
                        Ok(())
                    }
                    _ => Err(FieldError::new(
                        name,
                        FieldErrorCode::InvalidValue,
                        "published must be true or false",
                    )),
                },
            };
            if let Err(error) = result {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(Self(page, filter))
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

pub(crate) async fn list_posts(
    State(state): State<AppState>,
    headers: HeaderMap,
    PostListQuery(page, filter): PostListQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state.content.list_posts(&principal, &page, filter).await?,
    ))
}

pub(crate) async fn get_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state.content.post(&principal, id.as_uuid()).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdatePostInput {
    slug: String,
}

pub(crate) async fn update_post(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    Input(input): Input<UpdatePostInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .content
            .update_slug(&principal, id.as_uuid(), input.slug, &meta)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateRevisionInput {
    expected_revision_id: EntityId,
    title: String,
    #[serde(default)]
    excerpt: Option<String>,
    markdown_body: String,
    #[serde(default)]
    category_ids: Vec<EntityId>,
    #[serde(default)]
    tag_ids: Vec<EntityId>,
    #[serde(default)]
    cover_media_id: Option<EntityId>,
    #[serde(default)]
    is_featured: bool,
    #[serde(default)]
    seo_title: Option<String>,
    #[serde(default)]
    seo_description: Option<String>,
}

/// 保存修订：内容有变化时创建新修订（201），与当前编辑版本相同时返回当前版本（200）。
pub(crate) async fn create_revision(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    Input(input): Input<CreateRevisionInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let expected = input.expected_revision_id.as_uuid();
    let content: RevisionContent = RevisionFields {
        title: input.title,
        excerpt: input.excerpt,
        markdown_body: input.markdown_body,
        category_ids: input.category_ids,
        tag_ids: input.tag_ids,
        cover_media_id: input.cover_media_id,
        is_featured: input.is_featured,
        seo_title: input.seo_title,
        seo_description: input.seo_description,
    }
    .into();
    Ok(
        match state
            .content
            .create_revision(&principal, id.as_uuid(), expected, content, &meta)
            .await?
        {
            RevisionResult::Created(revision) => {
                let location = format!("/api/v1/admin/posts/{}/revisions/{}", id, revision.id);
                response::created_at(&location, revision)
            }
            RevisionResult::Unchanged(revision) => response::ok(revision),
        },
    )
}

pub(crate) async fn list_revisions(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
    PageQuery(page): PageQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state
            .content
            .revisions(&principal, id.as_uuid(), &page)
            .await?,
    ))
}

/// 文章下子资源的路径：`/posts/{id}/{collection}/{child_id}`。
pub(crate) struct ChildPath {
    post: Uuid,
    child: Uuid,
}

impl ChildPath {
    async fn parse<S: Send + Sync>(
        parts: &mut Parts,
        state: &S,
        child: &'static str,
    ) -> HttpResult<Self> {
        let Path(values) = Path::<Vec<(String, String)>>::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                ApiError::validation(vec![FieldError::new(
                    "id",
                    FieldErrorCode::InvalidFormat,
                    "id must be a hyphenated UUIDv7",
                )])
            })?;
        let value = |name: &str| {
            values
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or_default()
        };
        let mut errors = Vec::new();
        let post = parse_uuid("id", value("id"))
            .map_err(|e| errors.push(e))
            .ok();
        let child_id = parse_uuid(child, value(child))
            .map_err(|e| errors.push(e))
            .ok();
        match (post, child_id) {
            (Some(post), Some(child)) => Ok(Self { post, child }),
            _ => Err(ApiError::validation(errors)),
        }
    }
}

pub(crate) struct RevisionPath(ChildPath);
impl<S: Send + Sync> FromRequestParts<S> for RevisionPath {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        ChildPath::parse(parts, state, "revision_id")
            .await
            .map(Self)
    }
}

pub(crate) struct ReviewPath(ChildPath);
impl<S: Send + Sync> FromRequestParts<S> for ReviewPath {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        ChildPath::parse(parts, state, "review_id").await.map(Self)
    }
}

/// 只含分页参数的查询。
pub(crate) struct PageQuery(pub PageRequest);
impl<S: Send + Sync> FromRequestParts<S> for PageQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = parse_page(&pairs, &mut errors);
        if errors.is_empty() {
            Ok(Self(page))
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

pub(crate) async fn get_revision(
    State(state): State<AppState>,
    headers: HeaderMap,
    RevisionPath(path): RevisionPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .content
            .revision(&principal, path.post, path.child)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateReviewInput {
    revision_id: EntityId,
}

pub(crate) async fn create_review(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    Input(input): Input<CreateReviewInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let review = state
        .content
        .create_review(&principal, id.as_uuid(), input.revision_id.as_uuid(), &meta)
        .await?;
    let location = format!("/api/v1/admin/posts/{}/reviews/{}", id, review.id);
    Ok(response::created_at(&location, review))
}

pub(crate) async fn list_reviews(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
    PageQuery(page): PageQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::page(
        state
            .content
            .reviews(&principal, id.as_uuid(), &page)
            .await?,
    ))
}

pub(crate) async fn get_review(
    State(state): State<AppState>,
    headers: HeaderMap,
    ReviewPath(path): ReviewPath,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .content
            .review(&principal, path.post, path.child)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecideReviewInput {
    decision: String,
    #[serde(default)]
    comment: Option<String>,
}

pub(crate) async fn decide_review(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    ReviewPath(path): ReviewPath,
    Input(input): Input<DecideReviewInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state
            .content
            .decide_review(
                &principal,
                path.post,
                path.child,
                &input.decision,
                input.comment,
                &meta,
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublicationInput {
    revision_id: EntityId,
    expected_publication_version: i64,
}

pub(crate) async fn publish(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    Input(input): Input<PublicationInput>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::accepted(
        state
            .content
            .publish(
                &principal,
                id.as_uuid(),
                input.revision_id.as_uuid(),
                input.expected_publication_version,
                &meta,
            )
            .await?,
    ))
}

/// 撤下的查询参数：必填的 `expected_publication_version`。
pub(crate) struct UnpublishQuery(i64);
impl<S: Send + Sync> FromRequestParts<S> for UnpublishQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        const NAME: &str = "expected_publication_version";
        let pairs = query_pairs(parts, state).await?;
        let values: Vec<_> = pairs.iter().filter(|(key, _)| key == NAME).collect();
        let error = |code, message: &'static str| {
            ApiError::validation(vec![FieldError::new(NAME, code, message)])
        };
        match values.as_slice() {
            [] => Err(error(FieldErrorCode::Required, "parameter is required")),
            [(_, value)] => value
                .parse::<i64>()
                .ok()
                .filter(|version| *version >= 0)
                .map(Self)
                .ok_or_else(|| {
                    error(
                        FieldErrorCode::InvalidType,
                        "value must be a non-negative integer",
                    )
                }),
            _ => Err(ApiError::validation(vec![duplicate_parameter(NAME)])),
        }
    }
}

pub(crate) async fn unpublish(
    State(state): State<AppState>,
    ClientMeta(meta): ClientMeta,
    headers: HeaderMap,
    IdPath(id): IdPath,
    UnpublishQuery(version): UnpublishQuery,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::accepted(
        state
            .content
            .unpublish(&principal, id.as_uuid(), version, &meta)
            .await?,
    ))
}
