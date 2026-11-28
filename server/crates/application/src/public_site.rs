//! 公开 API、SSR 和媒体可见性共用同一已发布数据视图。
use crate::ports::{
    ByteStream, ExportedSite, MarkdownRenderer, ObjectStorage, PublicSiteRepository, StreamedObject,
};
use silva_domain::{
    media::MAX_UPLOAD_BYTES,
    site::{
        SNAPSHOT_SCHEMA_VERSION, SiteSnapshot, SnapshotExperience, SnapshotProfile,
        SnapshotTrending,
    },
};
use silva_shared::error::{AppError, Result};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct LiveSite {
    pub generation: i64,
    pub snapshot: SiteSnapshot,
    media: HashMap<Uuid, String>,
}

pub struct PublicSiteService {
    repository: Arc<dyn PublicSiteRepository>,
    storage: Option<Arc<dyn ObjectStorage>>,
    renderer: Arc<dyn MarkdownRenderer>,
    cache: Mutex<Option<Arc<LiveSite>>>,
}

impl PublicSiteService {
    pub fn new(
        repository: Arc<dyn PublicSiteRepository>,
        storage: Option<Arc<dyn ObjectStorage>>,
        renderer: Arc<dyn MarkdownRenderer>,
    ) -> Self {
        Self {
            repository,
            storage,
            renderer,
            cache: Mutex::new(None),
        }
    }

    /// 正常请求只查询版本；并发读到新版本时只由一个请求装载。
    pub async fn current(&self) -> Result<Arc<LiveSite>> {
        let generation = self.repository.generation().await?;
        let mut cache = self.cache.lock().await;
        if let Some(site) = cache.as_ref().filter(|site| site.generation == generation) {
            return Ok(site.clone());
        }
        let (generation, updated, exported) = self.repository.export().await?;
        let snapshot = self.snapshot(generation, updated, exported);
        snapshot.validate_series().map_err(anyhow::Error::msg)?;
        let media = snapshot
            .media
            .iter()
            .map(|m| (m.id, m.mime_type.clone()))
            .collect();
        let site = Arc::new(LiveSite {
            generation,
            snapshot,
            media,
        });
        *cache = Some(site.clone());
        Ok(site)
    }

    /// 组装快照；个人资料的 Markdown 在这里渲染。
    fn snapshot(
        &self,
        generation: i64,
        generated_at: chrono::DateTime<chrono::Utc>,
        site: ExportedSite,
    ) -> SiteSnapshot {
        let render = |markdown: Option<String>| {
            markdown
                .filter(|value| !value.trim().is_empty())
                .map(|value| self.renderer.render(&value, &HashMap::new()).html)
        };
        let profile = site.profile.map(|exported| {
            let profile = exported.profile;
            SnapshotProfile {
                display_name: profile.display_name,
                headline: profile.headline,
                bio_html: render(profile.bio_md),
                location: profile.location,
                avatar: exported.avatar,
                experiences: profile
                    .experiences
                    .into_iter()
                    .map(|experience| SnapshotExperience {
                        organization: experience.organization,
                        role: experience.role,
                        description_html: render(experience.description_md),
                        started_on: experience.started_on,
                        ended_on: experience.ended_on,
                    })
                    .collect(),
                skills: profile.skills,
                links: profile.links,
            }
        });
        SiteSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generation,
            generated_at,
            series: site.series,
            posts: site.posts,
            categories: site.categories,
            tags: site.tags,
            profile,
            ai_trending: SnapshotTrending::default(),
            media: site.media,
        }
    }
    /// 以流读取线上快照引用的媒体；未被引用时返回 None。返回 MIME 类型、大小与内容。
    pub async fn media(
        &self,
        site: &LiveSite,
        id: Uuid,
    ) -> Result<Option<(String, u64, ByteStream)>> {
        let Some(mime_type) = site.media.get(&id) else {
            return Ok(None);
        };
        let storage = self
            .storage
            .as_deref()
            .ok_or(AppError::StorageUnavailable)?;
        let key = self
            .repository
            .media_storage_key(id)
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("published media record is missing: {id}"))
            })?;
        match storage.stream(&key, MAX_UPLOAD_BYTES).await? {
            StreamedObject::Stream { size, body } => Ok(Some((mime_type.to_owned(), size, body))),
            other => Err(AppError::Internal(anyhow::anyhow!(
                "published media is unavailable: {id} ({})",
                describe(&other)
            ))),
        }
    }
}

fn describe(object: &StreamedObject) -> &'static str {
    match object {
        StreamedObject::Missing => "missing",
        StreamedObject::TooLarge { .. } => "too large",
        StreamedObject::Stream { .. } => "available",
    }
}
