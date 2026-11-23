pub mod credentials;
pub mod persistence;

use silva_application::identity::IdentityService;
use std::{sync::Arc, time::Duration};

pub async fn identity_service(
    pool: sqlx::PgPool,
    session_ttl: Duration,
) -> anyhow::Result<IdentityService> {
    Ok(IdentityService::new(
        Arc::new(persistence::PgIdentityRepository::new(pool)),
        Arc::new(credentials::CryptoCredentials::new().await?),
        session_ttl,
    ))
}
