pub mod content;
pub mod credentials;
pub mod image_inspection;
pub mod markdown;
pub mod media;
pub mod object_storage;
pub mod persistence;
pub mod profile;
mod public_site;
pub mod taxonomy;
pub mod tokens;

use silva_application::{
    content::ContentService, identity::IdentityService, media::MediaService, ports::ObjectStorage,
    public_site::PublicSiteService, taxonomy::TaxonomyService,
};
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

pub fn content_service(pool: sqlx::PgPool) -> ContentService {
    ContentService::new(
        Arc::new(content::PgContentRepository::new(pool)),
        Arc::new(markdown::CommonMarkRenderer),
    )
}

pub fn profile_service(pool: sqlx::PgPool) -> silva_application::profile::ProfileService {
    silva_application::profile::ProfileService::new(Arc::new(profile::PgProfileRepository::new(
        pool,
    )))
}

pub fn taxonomy_service(pool: sqlx::PgPool) -> TaxonomyService {
    TaxonomyService::new(Arc::new(taxonomy::PgTaxonomyRepository::new(pool)))
}

pub fn public_site_service(
    pool: sqlx::PgPool,
    storage: Option<Arc<dyn ObjectStorage>>,
) -> PublicSiteService {
    PublicSiteService::new(
        Arc::new(public_site::PgPublicSiteRepository::new(pool)),
        storage,
        Arc::new(markdown::CommonMarkRenderer),
    )
}
mod series;

pub fn series_service(pool: sqlx::PgPool) -> silva_application::series::SeriesService {
    silva_application::series::SeriesService::new(Arc::new(series::PgSeriesRepository(pool)))
}
