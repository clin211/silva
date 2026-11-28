mod content;
mod error;
mod http;
mod media;
mod metrics;
mod public;
mod response;
mod series;
mod taxonomy;
pub use error::ErrorCode;
pub use metrics::sample_metrics;
pub use silva_shared::error::FieldErrorCode;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    handler::Handler,
    middleware,
    routing::{delete, get, post, put},
};
use silva_application::{
    content::ContentService, identity::IdentityService, login_lockout::LoginLockout,
    media::MediaService, ports::ObjectStorage, profile::ProfileService,
    public_site::PublicSiteService, taxonomy::TaxonomyService,
};
use silva_infrastructure::{
    TokenSettings, content_service, identity_service, media_service, object_storage,
    profile_service, public_site_service, taxonomy_service,
};
use silva_shared::config::Config;
use sqlx::PgPool;
use std::sync::Arc;

/// 文章与修订请求正文的上限：512 KiB Markdown 加 JSON 转义与其他字段。
const CONTENT_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub(crate) struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub identity: Arc<IdentityService>,
    pub media: Arc<MediaService>,
    pub content: Arc<ContentService>,
    pub profile: Arc<ProfileService>,
    pub series: Arc<silva_application::series::SeriesService>,
    pub taxonomy: Arc<TaxonomyService>,
    pub public_site: Arc<PublicSiteService>,
    pub metrics: &'static silva_shared::metrics::Metrics,
    pub limiter: Arc<http::RateLimiter>,
}

/// 按配置构建路由。生产环境必须配置对象存储；开发与测试环境未配置时媒体写入和读取接口返回 503。
pub async fn build_router(config: Config, pool: PgPool) -> anyhow::Result<Router> {
    let storage = object_storage(&config)?;
    if storage.is_none() {
        if config.production {
            anyhow::bail!("SILVA_STORAGE_PROVIDER is required in production");
        }
        tracing::warn!("object storage is not configured; media uploads are unavailable");
    }
    build_router_with_storage(config, pool, storage).await
}

/// 使用指定的对象存储构建路由，供测试注入内存实现。
pub async fn build_router_with_storage(
    config: Config,
    pool: PgPool,
    storage: Option<Arc<dyn ObjectStorage>>,
) -> anyhow::Result<Router> {
    let media = Arc::new(media_service(pool.clone(), storage.clone(), &config));
    let content = Arc::new(content_service(pool.clone()));
    let taxonomy = Arc::new(taxonomy_service(pool.clone()));
    let public_site = Arc::new(public_site_service(pool.clone(), storage));
    let tokens = TokenSettings {
        secret: config.resolve_jwt_secret()?,
        access_ttl: config.access_token_ttl,
        refresh_ttl: config.refresh_token_ttl,
    };
    let identity = Arc::new(
        identity_service(pool.clone(), &tokens)
            .await?
            .with_login_lockout(LoginLockout::new(
                config.login_lockout_threshold,
                config.login_lockout_duration,
            )),
    );
    let state = AppState {
        config: Arc::new(config),
        series: Arc::new(silva_infrastructure::series_service(pool.clone())),
        profile: Arc::new(profile_service(pool.clone())),
        pool,
        identity,
        media,
        content,
        taxonomy,
        public_site,
        metrics: &silva_shared::metrics::METRICS,
        limiter: Arc::default(),
    };
    let router = if state.config.trust_proxy_ip_header {
        Router::new().route("/internal/site-snapshot", get(public::snapshot))
    } else {
        Router::new()
    };
    Ok(router
        .route("/health/live", get(http::live))
        .route("/metrics", get(metrics::endpoint))
        // 登录即创建会话；注销删除当前会话。
        .route("/api/v1/admin/sessions", post(http::login))
        .route("/api/v1/admin/sessions/current", delete(http::logout))
        // 用 refresh token 签发新的令牌对。
        .route("/api/v1/admin/tokens", post(http::refresh))
        .route(
            "/api/v1/admin/users",
            get(http::users).post(http::create_user),
        )
        .route("/api/v1/admin/users/profile", put(http::save_profile))
        // 静态路径 `me` 优先于 `{id}` 参数匹配，表示当前登录账户。
        .route("/api/v1/admin/users/me", get(http::user_info))
        .route(
            "/api/v1/admin/users/me/password",
            put(http::change_password),
        )
        .route(
            "/api/v1/admin/users/{id}",
            get(http::user)
                .patch(http::update_user)
                .delete(http::delete_user),
        )
        .route("/api/v1/admin/roles", get(http::roles))
        .route("/api/v1/admin/audit-logs", get(http::audit_logs))
        .route("/api/v1/admin/audit-logs/{id}", get(http::audit_log))
        // 单请求上传图片；PATCH 支持替代文本和旧版上传的确认。
        .route("/api/v1/admin/media", get(media::list).post(media::create))
        .route(
            "/api/v1/admin/media/{id}",
            get(media::get).patch(media::update).delete(media::delete),
        )
        .route("/api/v1/admin/media/{id}/content", get(media::content))
        .route(
            "/api/v1/admin/posts",
            get(content::list_posts)
                .post(content::create_post.layer(DefaultBodyLimit::max(CONTENT_BODY_LIMIT))),
        )
        .route(
            "/api/v1/admin/posts/category-counts",
            get(content::post_category_counts),
        )
        .route(
            "/api/v1/admin/posts/{id}",
            get(content::get_post)
                .patch(content::update_post)
                .delete(content::delete_post),
        )
        .route(
            "/api/v1/admin/posts/{id}/revisions",
            get(content::list_revisions)
                .post(content::create_revision.layer(DefaultBodyLimit::max(CONTENT_BODY_LIMIT))),
        )
        .route(
            "/api/v1/admin/posts/{id}/revisions/{revision_id}",
            get(content::get_revision),
        )
        .route(
            "/api/v1/admin/posts/{id}/publication",
            put(content::publish).delete(content::unpublish),
        )
        .route(
            "/api/v1/admin/series",
            get(series::list).post(series::create),
        )
        .route(
            "/api/v1/admin/series/{id}",
            get(series::get).patch(series::patch).delete(series::delete),
        )
        .route(
            "/api/v1/admin/series/{id}/publication",
            put(series::publish).delete(series::unpublish),
        )
        .route(
            "/api/v1/admin/series/{id}/outline",
            put(series::outline.layer(DefaultBodyLimit::max(CONTENT_BODY_LIMIT))),
        )
        .route("/api/v1/admin/series/{id}/items", post(series::append))
        .route(
            "/api/v1/admin/series/{id}/items/{post_id}",
            delete(series::remove),
        )
        .route(
            "/api/v1/admin/categories",
            get(taxonomy::list_categories).post(taxonomy::create_category),
        )
        .route(
            "/api/v1/admin/categories/{id}",
            get(taxonomy::get_category)
                .patch(taxonomy::update_category)
                .delete(taxonomy::delete_category),
        )
        .route(
            "/api/v1/admin/tags",
            get(taxonomy::list_tags).post(taxonomy::create_tag),
        )
        .route(
            "/api/v1/admin/tags/{id}",
            get(taxonomy::get_tag)
                .patch(taxonomy::update_tag)
                .delete(taxonomy::delete_tag),
        )
        .route("/api/v1/public/site", get(public::site))
        .route("/api/v1/public/site-snapshot", get(public::snapshot))
        .route("/api/v1/public/posts", get(public::posts))
        .route("/api/v1/public/posts/{slug}", get(public::post))
        .route("/api/v1/public/categories", get(public::categories))
        .route("/api/v1/public/tags", get(public::tags))
        .route("/api/v1/public/profile", get(public::profile))
        .route("/api/v1/public/ai-trending", get(public::ai_trending))
        .route(
            "/api/v1/public/ai-trending/{slug}",
            get(public::ai_repository),
        )
        .route(
            "/api/v1/public/ai-trending/{slug}/snapshots",
            get(public::ai_repository),
        )
        .route("/api/v1/public/media/{id}", get(public::media))
        .fallback(|| async { error::ApiError::from(silva_shared::error::AppError::NotFound) })
        .method_not_allowed_fallback(http::method_not_allowed)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), http::context))
        .with_state(state))
}
