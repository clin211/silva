pub mod credentials;
pub mod persistence;
pub mod tokens;

use silva_application::identity::IdentityService;
use std::sync::Arc;
pub use tokens::TokenSettings;

pub async fn identity_service(
    pool: sqlx::PgPool,
    tokens: &TokenSettings,
) -> anyhow::Result<IdentityService> {
    Ok(IdentityService::new(
        Arc::new(persistence::PgIdentityRepository::new(pool)),
        Arc::new(credentials::CryptoCredentials::new().await?),
        Arc::new(tokens::JwtTokens::new(tokens)),
    ))
}
