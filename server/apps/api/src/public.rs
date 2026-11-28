//! 公开 API 固定一次读取的已发布版本，并通过 X-Site-Generation 返回版本号。

use crate::{
    AppState,
    error::ApiError,
    http::{HttpResult, IdPath},
    response,
};
use axum::{
    body::Body,
    extract::{FromRequestParts, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use silva_application::public_site::LiveSite;
use silva_domain::{
    content::Heading,
    site::{SnapshotImage, SnapshotPost, SnapshotProfile},
};
use silva_shared::{
    error::{AppError, Conflict, FieldError, FieldErrorCode},
    pagination::{Page, PageRequest},
};
use std::sync::Arc;

const GENERATION_HEADER: &str = "x-site-generation";
const EXPECTED_HEADER: &str = "x-expected-generation";
/// 读取当前线上站点；请求带 `X-Expected-Generation` 且与线上不一致时返回 409。
async fn live(state: &AppState, headers: &HeaderMap) -> HttpResult<Arc<LiveSite>> {
    let expected = match headers.get(EXPECTED_HEADER) {
        None => None,
        Some(value) => Some(
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
                .filter(|generation| *generation >= 0)
                .ok_or_else(|| {
                    ApiError::validation(vec![FieldError::new(
                        "X-Expected-Generation",
                        FieldErrorCode::InvalidType,
                        "header must be a non-negative integer",
                    )])
                })?,
        ),
    };
    let site = state.public_site.current().await?;
    if expected.is_some_and(|expected| expected != site.generation) {
        let mut error = ApiError::from(AppError::Conflict(Conflict::SiteGenerationChanged));
        error.generation = Some(site.generation);
        return Err(error);
    }
    Ok(site)
}

fn with_generation(mut response: Response, generation: i64) -> Response {
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(GENERATION_HEADER, HeaderValue::from(generation));
    response
}

#[derive(Serialize)]
struct TaxonomyLabel<'a> {
    slug: &'a str,
    name: &'a str,
}

/// 文章列表项。
#[derive(Serialize)]
struct PostCard<'a> {
    slug: &'a str,
    title: &'a str,
    excerpt: Option<&'a str>,
    categories: Vec<TaxonomyLabel<'a>>,
    tags: Vec<TaxonomyLabel<'a>>,
    featured: bool,
    cover: Option<&'a SnapshotImage>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    published_at: chrono::DateTime<chrono::Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// 文章详情：列表项加正文 HTML、目录与 SEO 字段。
#[derive(Serialize)]
struct PostDetail<'a> {
    #[serde(flatten)]
    card: PostCard<'a>,
    html: &'a str,
    headings: &'a [Heading],
    seo_title: Option<&'a str>,
    seo_description: Option<&'a str>,
}

fn card<'a>(site: &'a LiveSite, post: &'a SnapshotPost) -> PostCard<'a> {
    let snapshot = &site.snapshot;
    PostCard {
        slug: &post.slug,
        title: &post.title,
        excerpt: post.excerpt.as_deref(),
        categories: post
            .categories
            .iter()
            .filter_map(|slug| snapshot.categories.iter().find(|c| &c.slug == slug))
            .map(|c| TaxonomyLabel {
                slug: &c.slug,
                name: &c.name,
            })
            .collect(),
        tags: post
            .tags
            .iter()
            .filter_map(|slug| snapshot.tags.iter().find(|t| &t.slug == slug))
            .map(|t| TaxonomyLabel {
                slug: &t.slug,
                name: &t.name,
            })
            .collect(),
        featured: post.featured,
        cover: post.cover.as_ref(),
        published_at: post.published_at,
        updated_at: post.updated_at,
    }
}

#[derive(Serialize)]
struct SiteSummary<'a> {
    generation: i64,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    generated_at: chrono::DateTime<chrono::Utc>,
    featured_posts: Vec<PostCard<'a>>,
    latest_posts: Vec<PostCard<'a>>,
    has_profile: bool,
    ai_trending_data_date: Option<chrono::NaiveDate>,
}

/// SSR 的完整公开数据。浏览器、代理和服务端均需在复用前重验证。
pub(crate) async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let tag = format!("\"site-v1-{}\"", site.generation);
    let matched = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == tag
        });
    let mut response = if matched {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        response::ok(&site.snapshot)
    };
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&format!("W/{tag}")).expect("integer generation yields valid ETag"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(with_generation(response, site.generation))
}

/// 首页所需的站点数据。
pub(crate) async fn site(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let posts = &site.snapshot.posts;
    let body = SiteSummary {
        generation: site.generation,
        generated_at: site.snapshot.generated_at,
        featured_posts: posts
            .iter()
            .filter(|p| p.featured)
            .map(|p| card(&site, p))
            .collect(),
        latest_posts: posts.iter().take(6).map(|p| card(&site, p)).collect(),
        has_profile: site.snapshot.profile.is_some(),
        ai_trending_data_date: site.snapshot.ai_trending.data_date,
    };
    Ok(with_generation(response::ok(body), site.generation))
}

/// 公开文章列表查询：分页加按 slug 筛选的 `category`、`tag`。
pub(crate) struct PublicPostQuery {
    page: PageRequest,
    category: Option<String>,
    tag: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for PublicPostQuery {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> HttpResult<Self> {
        let pairs = crate::http::query_pairs(parts, state).await?;
        let mut errors = Vec::new();
        let page = crate::http::parse_page(&pairs, &mut errors);
        let mut filters: [Option<String>; 2] = Default::default();
        for (key, value) in pairs {
            let index = match key.as_str() {
                "category" => 0,
                "tag" => 1,
                _ => continue,
            };
            if filters[index].is_some() {
                errors.push(crate::http::duplicate_parameter(&key));
            } else {
                filters[index] = Some(value);
            }
        }
        let [category, tag] = filters;
        if errors.is_empty() {
            Ok(Self {
                page,
                category,
                tag,
            })
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

pub(crate) async fn posts(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: PublicPostQuery,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let matching: Vec<_> = site
        .snapshot
        .posts
        .iter()
        .filter(|post| {
            query
                .category
                .as_ref()
                .is_none_or(|slug| post.categories.contains(slug))
        })
        .filter(|post| {
            query
                .tag
                .as_ref()
                .is_none_or(|slug| post.tags.contains(slug))
        })
        .collect();
    let total = matching.len() as i64;
    let items = matching
        .into_iter()
        .skip(query.page.offset() as usize)
        .take(query.page.per_page as usize)
        .map(|post| card(&site, post))
        .collect();
    let page = Page {
        items,
        pagination: query.page.metadata(total),
    };
    Ok(with_generation(response::page(page), site.generation))
}

pub(crate) async fn post(
    State(state): State<AppState>,
    headers: HeaderMap,
    slug: Result<Path<String>, axum::extract::rejection::PathRejection>,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let Path(slug) = slug.map_err(|_| AppError::NotFound)?;
    let post = site
        .snapshot
        .posts
        .iter()
        .find(|post| post.slug == slug)
        .ok_or(AppError::NotFound)?;
    let body = PostDetail {
        card: card(&site, post),
        html: &post.html,
        headings: &post.headings,
        seo_title: post.seo_title.as_deref(),
        seo_description: post.seo_description.as_deref(),
    };
    Ok(with_generation(response::ok(body), site.generation))
}

#[derive(Serialize)]
struct TaxonomyCount<'a> {
    slug: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<Option<&'a str>>,
    post_count: usize,
}

pub(crate) async fn categories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let items: Vec<_> = site
        .snapshot
        .categories
        .iter()
        .map(|category| TaxonomyCount {
            slug: &category.slug,
            name: &category.name,
            description: Some(category.description.as_deref()),
            post_count: site
                .snapshot
                .posts
                .iter()
                .filter(|post| post.categories.contains(&category.slug))
                .count(),
        })
        .collect();
    Ok(with_generation(response::ok(items), site.generation))
}

pub(crate) async fn tags(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let items: Vec<_> = site
        .snapshot
        .tags
        .iter()
        .map(|tag| TaxonomyCount {
            slug: &tag.slug,
            name: &tag.name,
            description: None,
            post_count: site
                .snapshot
                .posts
                .iter()
                .filter(|post| post.tags.contains(&tag.slug))
                .count(),
        })
        .collect();
    Ok(with_generation(response::ok(items), site.generation))
}

pub(crate) async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let profile: &SnapshotProfile = site.snapshot.profile.as_ref().ok_or(AppError::NotFound)?;
    Ok(with_generation(response::ok(profile), site.generation))
}

#[derive(Serialize)]
struct Trending<'a> {
    data_date: Option<chrono::NaiveDate>,
    repositories: &'a [serde_json::Value],
    featured: &'a [serde_json::Value],
}

/// AI 榜单。采集实现前没有成功批次，返回空榜单与 `data_date: null`。
pub(crate) async fn ai_trending(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let trending = &site.snapshot.ai_trending;
    let body = Trending {
        data_date: trending.data_date,
        repositories: &trending.repositories,
        featured: &[],
    };
    Ok(with_generation(response::ok(body), site.generation))
}

/// AI 仓库详情与快照；采集实现前没有任何仓库。
pub(crate) async fn ai_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    live(&state, &headers).await?;
    Err(AppError::NotFound.into())
}

/// 线上快照引用的媒体；其他 ID 返回 404。
pub(crate) async fn media(
    State(state): State<AppState>,
    headers: HeaderMap,
    IdPath(id): IdPath,
) -> HttpResult<Response> {
    let site = live(&state, &headers).await?;
    let (mime_type, size, body) = state
        .public_site
        .media(&site, id.as_uuid())
        .await?
        .ok_or(AppError::NotFound)?;
    let mut response = Body::from_stream(body).into_response();
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&mime_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    Ok(with_generation(response, site.generation))
}
