pub mod credentials;
pub mod image_inspection;
pub mod media;
pub mod object_storage;
pub mod persistence;
pub mod tokens;

use silva_application::{identity::IdentityService, media::MediaService, ports::ObjectStorage};
use silva_shared::config::Config;
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

/// 按配置创建对象存储；未配置时返回 None。
pub fn object_storage(config: &Config) -> anyhow::Result<Option<Arc<dyn ObjectStorage>>> {
    Ok(match &config.storage {
        Some(storage) => Some(Arc::new(object_storage::OpendalStorage::new(storage)?)),
        None => None,
    })
}

/// 组装媒体用例；`storage` 由调用方提供，测试可注入内存实现。
pub fn media_service(
    pool: sqlx::PgPool,
    storage: Option<Arc<dyn ObjectStorage>>,
    config: &Config,
) -> MediaService {
    MediaService::new(
        Arc::new(media::PgMediaRepository::new(pool)),
        storage,
        Arc::new(image_inspection::DecodingInspector::default()),
        config.media_upload_ttl,
    )
}
