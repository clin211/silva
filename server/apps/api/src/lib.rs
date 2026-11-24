mod error;
mod http;
mod metrics;
mod response;
pub use error::ErrorCode;
pub use metrics::sample_metrics;
pub use silva_shared::error::FieldErrorCode;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use silva_application::{identity::IdentityService, login_lockout::LoginLockout};
use silva_infrastructure::{TokenSettings, identity_service};
use silva_shared::config::Config;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub identity: Arc<IdentityService>,
    pub metrics: &'static silva_shared::metrics::Metrics,
    pub limiter: Arc<http::RateLimiter>,
}

pub async fn build_router(config: Config, pool: PgPool) -> anyhow::Result<Router> {
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
        metrics: &silva_shared::metrics::METRICS,
        limiter: Arc::default(),
    };
    Ok(Router::new()
        .route("/health/live", get(http::live))
        .route("/metrics", get(metrics::endpoint))
        .route("/api/v1/admin/auth/login", post(http::login))
        .route("/api/v1/admin/auth/refresh", post(http::refresh))
        .route("/api/v1/admin/auth/logout", post(http::logout))
        .route("/api/v1/admin/auth/password", post(http::change_password))
        .route(
            "/api/v1/admin/users",
            get(http::users).post(http::create_user),
        )
        // 静态路径优先于 `{id}` 参数匹配。
        .route("/api/v1/admin/users/info", get(http::user_info))
        .route(
            "/api/v1/admin/users/{id}",
            get(http::user).patch(http::update_user),
        )
        .route("/api/v1/admin/roles", get(http::roles))
        .route("/api/v1/admin/audit-logs", get(http::audit_logs))
        .route("/api/v1/admin/audit-logs/{id}", get(http::audit_log))
        .fallback(http::not_found)
        .method_not_allowed_fallback(http::method_not_allowed)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), http::context))
        .with_state(state))
}
