mod error;
mod http;
mod media;
mod metrics;
mod response;
pub use error::ErrorCode;
pub use metrics::sample_metrics;
pub use silva_shared::error::FieldErrorCode;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post, put},
};
use silva_application::{
    identity::IdentityService, login_lockout::LoginLockout, media::MediaService,
    ports::ObjectStorage,
};
use silva_infrastructure::{TokenSettings, identity_service, media_service, object_storage};
use silva_shared::config::Config;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub identity: Arc<IdentityService>,
    pub media: Arc<MediaService>,
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
    let media = Arc::new(media_service(pool.clone(), storage, &config));
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
        pool,
        identity,
        media,
        metrics: &silva_shared::metrics::METRICS,
        limiter: Arc::default(),
    };
    Ok(Router::new()
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
        // 静态路径 `me` 优先于 `{id}` 参数匹配，表示当前登录账户。
        .route("/api/v1/admin/users/me", get(http::user_info))
        .route(
            "/api/v1/admin/users/me/password",
            put(http::change_password),
        )
        .route(
            "/api/v1/admin/users/{id}",
            get(http::user).patch(http::update_user),
        )
        .route("/api/v1/admin/roles", get(http::roles))
        .route("/api/v1/admin/audit-logs", get(http::audit_logs))
        .route("/api/v1/admin/audit-logs/{id}", get(http::audit_log))
        // 申请直传即创建媒体记录；确认上传通过 PATCH 修改状态。
        .route("/api/v1/admin/media", get(media::list).post(media::create))
        .route(
            "/api/v1/admin/media/{id}",
            get(media::get).patch(media::update).delete(media::delete),
        )
        .route("/api/v1/admin/media/{id}/content", get(media::content))
        .fallback(http::not_found)
        .method_not_allowed_fallback(http::method_not_allowed)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), http::context))
        .with_state(state))
}
