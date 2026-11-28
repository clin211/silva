//! 分类与标签管理接口。

use crate::{
    AppState,
    http::{ClientMeta, HttpResult, IdPath, Input, authorize},
    response,
};
use axum::{extract::State, http::HeaderMap, response::Response};
use serde::{Deserialize, Serialize};
use silva_application::ports::{TaxonomyItem, TaxonomyKind};
use silva_domain::{content::TaxonomyInput, site::SiteVersion};

const CATEGORY: TaxonomyKind = TaxonomyKind::Category;
const TAG: TaxonomyKind = TaxonomyKind::Tag;

fn collection(kind: TaxonomyKind) -> &'static str {
    match kind {
        TaxonomyKind::Category => "categories",
        TaxonomyKind::Tag => "tags",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateCategoryInput {
    slug: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateTagInput {
    slug: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateCategoryInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateTagInput {
    #[serde(default)]
    name: Option<String>,
}

/// 更新结果：条目本身，以及影响公开内容时的版本号（否则为 null）。
#[derive(Serialize)]
struct Updated {
    item: TaxonomyItem,
    publication: Option<SiteVersion>,
}

async fn list(state: AppState, headers: HeaderMap, kind: TaxonomyKind) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(state.taxonomy.list(&principal, kind).await?))
}

async fn get(
    state: AppState,
    headers: HeaderMap,
    id: IdPath,
    kind: TaxonomyKind,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    Ok(response::ok(
        state.taxonomy.get(&principal, kind, id.0.as_uuid()).await?,
    ))
}

async fn create(
    state: AppState,
    meta: ClientMeta,
    headers: HeaderMap,
    kind: TaxonomyKind,
    slug: String,
    input: TaxonomyInput,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let item = state
        .taxonomy
        .create(&principal, kind, slug, input, &meta.0)
        .await?;
    let id = match &item {
        TaxonomyItem::Category(category) => category.id,
        TaxonomyItem::Tag(tag) => tag.id,
    };
    let location = format!("/api/v1/admin/{}/{id}", collection(kind));
    Ok(response::created_at(&location, item))
}

async fn update(
    state: AppState,
    meta: ClientMeta,
    headers: HeaderMap,
    id: IdPath,
    kind: TaxonomyKind,
    name: Option<String>,
    description: Option<String>,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    let (item, publication) = state
        .taxonomy
        .update(&principal, kind, id.0.as_uuid(), name, description, &meta.0)
        .await?;
    Ok(response::ok(Updated { item, publication }))
}

async fn delete(
    state: AppState,
    meta: ClientMeta,
    headers: HeaderMap,
    id: IdPath,
    kind: TaxonomyKind,
) -> HttpResult<Response> {
    let principal = authorize(&state, &headers).await?;
    state
        .taxonomy
        .delete(&principal, kind, id.0.as_uuid(), &meta.0)
        .await?;
    Ok(response::no_content())
}

pub(crate) async fn list_categories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    list(state, headers, CATEGORY).await
}

pub(crate) async fn list_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    list(state, headers, TAG).await
}

pub(crate) async fn get_category(
    State(state): State<AppState>,
    headers: HeaderMap,
    id: IdPath,
) -> HttpResult<Response> {
    get(state, headers, id, CATEGORY).await
}

pub(crate) async fn get_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    id: IdPath,
) -> HttpResult<Response> {
    get(state, headers, id, TAG).await
}

pub(crate) async fn create_category(
    State(state): State<AppState>,
    meta: ClientMeta,
    headers: HeaderMap,
    Input(input): Input<CreateCategoryInput>,
) -> HttpResult<Response> {
    let taxonomy = TaxonomyInput {
        name: input.name,
        description: input.description,
    };
    create(state, meta, headers, CATEGORY, input.slug, taxonomy).await
}

pub(crate) async fn create_tag(
    State(state): State<AppState>,
    meta: ClientMeta,
    headers: HeaderMap,
    Input(input): Input<CreateTagInput>,
) -> HttpResult<Response> {
    let taxonomy = TaxonomyInput {
        name: input.name,
        description: None,
    };
    create(state, meta, headers, TAG, input.slug, taxonomy).await
}

pub(crate) async fn update_category(
    State(state): State<AppState>,
    meta: ClientMeta,
    headers: HeaderMap,
    id: IdPath,
    Input(input): Input<UpdateCategoryInput>,
) -> HttpResult<Response> {
    update(
        state,
        meta,
        headers,
        id,
        CATEGORY,
        input.name,
        input.description,
    )
    .await
}

pub(crate) async fn update_tag(
    State(state): State<AppState>,
    meta: ClientMeta,
    headers: HeaderMap,
    id: IdPath,
    Input(input): Input<UpdateTagInput>,
) -> HttpResult<Response> {
    update(state, meta, headers, id, TAG, input.name, None).await
}

pub(crate) async fn delete_category(
    State(state): State<AppState>,
    meta: ClientMeta,
    headers: HeaderMap,
    id: IdPath,
) -> HttpResult<Response> {
    delete(state, meta, headers, id, CATEGORY).await
}

pub(crate) async fn delete_tag(
    State(state): State<AppState>,
    meta: ClientMeta,
    headers: HeaderMap,
    id: IdPath,
) -> HttpResult<Response> {
    delete(state, meta, headers, id, TAG).await
}
