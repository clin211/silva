pub mod content;
pub mod credentials;
pub mod image_inspection;
pub mod markdown;
pub mod media;
pub mod object_storage;
pub mod persistence;
pub mod profile;
pub mod publishing;
pub mod site_builder;
pub mod taxonomy;
pub mod tokens;

use silva_application::{
    content::ContentService,
    identity::IdentityService,
    media::MediaService,
    ports::{ObjectStorage, SiteBuilder},
    public_site::PublicSiteService,
    publishing::{PublishSettings, PublishingService},
    taxonomy::TaxonomyService,
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

/// 按配置创建站点构建器；未配置 `SILVA_SITE_BUILDER_DIR` 时返回 None。
pub fn site_builder(config: &Config) -> Option<Arc<dyn SiteBuilder>> {
    config.site_build.as_ref().map(|build| {
        Arc::new(site_builder::AstroSiteBuilder {
            website_dir: build.website_dir.clone(),
            command: build.command.clone(),
            site_url: build.site_url.clone(),
            timeout: build.timeout,
        }) as Arc<dyn SiteBuilder>
    })
}

/// 组装发布用例；存储与构建器由调用方提供，测试可注入替身。
pub fn publishing_service(
    pool: sqlx::PgPool,
    storage: Option<Arc<dyn ObjectStorage>>,
    builder: Option<Arc<dyn SiteBuilder>>,
) -> PublishingService {
    PublishingService::new(
        Arc::new(publishing::PgPublishingRepository::new(pool)),
        storage,
        builder,
        Arc::new(markdown::CommonMarkRenderer),
        PublishSettings::default(),
    )
}

pub fn public_site_service(
    pool: sqlx::PgPool,
    storage: Option<Arc<dyn ObjectStorage>>,
) -> PublicSiteService {
    PublicSiteService::new(
        Arc::new(publishing::PgPublishingRepository::new(pool)),
        storage,
    )
}
