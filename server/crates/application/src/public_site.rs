//! 公开站点读取：Public API 与静态站点入口都只读取当前 `active_generation` 的不可变快照和产物。
//!
//! 每个请求解析一次线上指针；同一 generation 的快照与清单不可变，按 generation 缓存在进程内。

use crate::{
    ports::{
        ActiveSite, ByteStream, ObjectStorage, PublishingRepository, StoredObject, StreamedObject,
    },
    publishing::sha256_hex,
};
use silva_domain::{
    media::{MAX_UPLOAD_BYTES, storage_key},
    site::{ArtifactFile, ArtifactManifest, SNAPSHOT_SCHEMA_VERSION, SiteSnapshot},
};
use silva_shared::error::{AppError, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

/// 读取快照与清单的大小上限。
const SNAPSHOT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 单个产物文件的大小上限。
const ARTIFACT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// 一个已上线 generation 的快照、清单与索引。
pub struct LiveSite {
    pub generation: i64,
    pub snapshot: SiteSnapshot,
    artifact_prefix: String,
    files: HashMap<String, ArtifactFile>,
    media: HashMap<Uuid, String>,
}

impl LiveSite {
    /// 清单中的产物文件；不存在时返回 None。
    pub fn file(&self, path: &str) -> Option<&ArtifactFile> {
        self.files.get(path)
    }

    /// 当前线上快照引用的媒体的 MIME 类型；未被引用时返回 None。
    pub fn media_type(&self, id: Uuid) -> Option<&str> {
        self.media.get(&id).map(String::as_str)
    }
}

pub struct PublicSiteService {
    repository: Arc<dyn PublishingRepository>,
    storage: Option<Arc<dyn ObjectStorage>>,
    cache: Mutex<Option<Arc<LiveSite>>>,
}

impl PublicSiteService {
    pub fn new(
        repository: Arc<dyn PublishingRepository>,
        storage: Option<Arc<dyn ObjectStorage>>,
    ) -> Self {
        Self {
            repository,
            storage,
            cache: Mutex::new(None),
        }
    }

    /// 当前线上站点；尚未上线返回 `SiteNotReady`。
    #[tracing::instrument(name = "public_site.current", skip_all)]
    pub async fn current(&self) -> Result<Arc<LiveSite>> {
        let active = self
            .repository
            .active_site()
            .await?
            .ok_or(AppError::SiteNotReady)?;
        if let Some(site) = self.cached(active.generation) {
            return Ok(site);
        }
        let site = Arc::new(self.load(&active).await?);
        *self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(site.clone());
        Ok(site)
    }

    fn cached(&self, generation: i64) -> Option<Arc<LiveSite>> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|site| site.generation == generation)
            .cloned()
    }

    async fn load(&self, active: &ActiveSite) -> Result<LiveSite> {
        let storage = self
            .storage
            .as_deref()
            .ok_or(AppError::StorageUnavailable)?;
        let manifest: ArtifactManifest =
            parse(&read_all(storage, &active.manifest_key, SNAPSHOT_MAX_BYTES).await?)?;
        let prefix = active
            .manifest_key
            .strip_suffix("manifest.json")
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("unexpected manifest key")))?;
        let snapshot_bytes = read_all(
            storage,
            &format!("{prefix}snapshot.json"),
            SNAPSHOT_MAX_BYTES,
        )
        .await?;
        if sha256_hex(&snapshot_bytes) != manifest.snapshot_sha256 {
            return Err(AppError::Internal(anyhow::anyhow!(
                "snapshot digest does not match manifest for generation {}",
                active.generation
            )));
        }
        let snapshot: SiteSnapshot = parse(&snapshot_bytes)?;
        if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION
            || snapshot.generation != active.generation
            || manifest.generation != active.generation
        {
            return Err(AppError::Internal(anyhow::anyhow!(
                "snapshot or manifest does not belong to generation {}",
                active.generation
            )));
        }
        Ok(LiveSite {
            generation: active.generation,
            media: snapshot
                .media
                .iter()
                .map(|media| (media.id, media.mime_type.clone()))
                .collect(),
            files: manifest
                .files
                .into_iter()
                .map(|file| (file.path.clone(), file))
                .collect(),
            artifact_prefix: active.artifact_prefix.clone(),
            snapshot,
        })
    }

    /// 以流读取产物文件；文件不在清单中时返回 None。
    pub async fn artifact(
        &self,
        site: &LiveSite,
        path: &str,
    ) -> Result<Option<(ArtifactFile, ByteStream)>> {
        let Some(file) = site.file(path) else {
            return Ok(None);
        };
        let storage = self
            .storage
            .as_deref()
            .ok_or(AppError::StorageUnavailable)?;
        let key = format!("{}{path}", site.artifact_prefix);
        let content = match storage.stream(&key, ARTIFACT_MAX_BYTES).await? {
            StreamedObject::Stream { body, .. } => body,
            other => {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "artifact listed in manifest is unavailable: {key} ({})",
                    describe(&other)
                )));
            }
        };
        Ok(Some((file.clone(), content)))
    }

    /// 以流读取线上快照引用的媒体；未被引用时返回 None。返回 MIME 类型、大小与内容。
    pub async fn media(
        &self,
        site: &LiveSite,
        id: Uuid,
    ) -> Result<Option<(String, u64, ByteStream)>> {
        let Some(mime_type) = site.media_type(id) else {
            return Ok(None);
        };
        let storage = self
            .storage
            .as_deref()
            .ok_or(AppError::StorageUnavailable)?;
        match storage.stream(&storage_key(id), MAX_UPLOAD_BYTES).await? {
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

async fn read_all(storage: &dyn ObjectStorage, key: &str, max: u64) -> Result<Vec<u8>> {
    match storage.read(key, max).await? {
        StoredObject::Bytes(bytes) => Ok(bytes),
        StoredObject::Missing => Err(AppError::Internal(anyhow::anyhow!(
            "published object is missing: {key}"
        ))),
        StoredObject::TooLarge { size } => Err(AppError::Internal(anyhow::anyhow!(
            "published object is too large: {key} ({size} bytes)"
        ))),
    }
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|error| AppError::Internal(error.into()))
}
