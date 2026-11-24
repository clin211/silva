//! 媒体上传用例：申请直传、确认与校验、读取、替代文本维护、删除和过期清理。
//!
//! 授权规则：持有 `media.manage` 可操作全部媒体；只持有 `media.manage_own` 时只能看到并操作
//! 自己上传的媒体，访问他人媒体按不存在处理（404），避免泄露媒体 ID 是否存在。

use crate::ports::{
    ByteStream, ImageInspector, MediaActor, MediaFilter, MediaRecord, MediaRepository, NewMedia,
    ObjectStorage, PresignedUpload, PutOutcome, RequestMeta, StoredObject, StreamedObject,
};
use chrono::{DateTime, Utc};
use silva_domain::{
    identity::Principal,
    media::{
        MAX_UPLOAD_BYTES, Media, MediaStatus, RejectionReason, UploadRequest, VerifiedImage,
        normalize_alt_text, storage_key, upload_key,
    },
};
use silva_shared::{
    error::{AppError, Conflict, FieldErrorCode, Result},
    ids,
    pagination::{Page, PageRequest},
};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

/// 管理全部媒体的权限。
pub const MEDIA_MANAGE: &str = "media.manage";
/// 管理本人上传媒体的权限。
pub const MEDIA_MANAGE_OWN: &str = "media.manage_own";
/// 过期的 `pending` 记录在截止时间后再保留的时长，避免与临近截止的确认请求竞争。
pub const EXPIRED_UPLOAD_GRACE: Duration = Duration::from_secs(3600);
/// `rejected` 记录保留的时长，供上传者查看拒绝原因。
pub const REJECTED_RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);
/// 每轮清理最多处理的记录数。
const PURGE_BATCH: i64 = 100;
/// 同时进行的上传确认数。每个确认最多在内存中持有约两份 10 MiB 的文件内容，
/// 在下载之前限流，避免大量并发确认占满内存。
const CONCURRENT_CONFIRMATIONS: usize = 4;

/// 一轮过期媒体清理的结果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// 已删除的记录数。
    pub purged: u64,
    /// 清理失败、留待后续批次的记录数。
    pub failed: u64,
}

/// 已校验媒体的文件内容；`body` 按需从对象存储传输。
pub struct MediaContent {
    pub media: Media,
    pub size: u64,
    pub body: ByteStream,
}

/// 申请直传的结果：新建的媒体记录和浏览器直传请求。
pub struct UploadTicket {
    pub media: Media,
    pub upload: PresignedUpload,
}

/// `PATCH /media/{id}` 的输入；字段为 None 时保持原值。
#[derive(Default)]
pub struct MediaUpdate {
    /// 只接受 `uploaded`，表示浏览器已完成直传、请求服务端校验。
    pub status: Option<String>,
    /// 替代文本；去空白后为空表示清除。
    pub alt_text: Option<String>,
}

pub struct MediaService {
    repository: Arc<dyn MediaRepository>,
    storage: Option<Arc<dyn ObjectStorage>>,
    inspector: Arc<dyn ImageInspector>,
    upload_ttl: Duration,
    confirmations: tokio::sync::Semaphore,
}

impl MediaService {
    /// `storage` 为 None 时媒体接口返回 `StorageUnavailable`，列表与详情仍可读取。
    pub fn new(
        repository: Arc<dyn MediaRepository>,
        storage: Option<Arc<dyn ObjectStorage>>,
        inspector: Arc<dyn ImageInspector>,
        upload_ttl: Duration,
    ) -> Self {
        Self {
            repository,
            storage,
            inspector,
            upload_ttl,
            confirmations: tokio::sync::Semaphore::new(CONCURRENT_CONFIRMATIONS),
        }
    }

    fn storage(&self) -> Result<&dyn ObjectStorage> {
        self.storage.as_deref().ok_or(AppError::StorageUnavailable)
    }

    /// 申请直传：校验声明信息，签发临时键的 PUT 地址并创建 `pending` 记录。
    #[tracing::instrument(name = "media.create_upload", skip_all)]
    pub async fn create_upload(
        &self,
        principal: &Principal,
        request: UploadRequest,
        meta: &RequestMeta,
    ) -> Result<UploadTicket> {
        require_any(principal)?;
        let request = request.validate()?;
        let storage = self.storage()?;
        let id = ids::new_id();
        let key = upload_key(id);
        let ttl = chrono::Duration::from_std(self.upload_ttl)
            .map_err(|error| AppError::Internal(error.into()))?;
        // 先计算截止时间再签名，签名地址不会晚于记录中的截止时间之前失效。
        let upload_expires_at = Utc::now() + ttl;
        let upload = storage
            .presign_put(
                &key,
                &request.declared_mime_type,
                request.size_bytes,
                self.upload_ttl,
            )
            .await?;
        let media = self
            .repository
            .create(
                &NewMedia {
                    id,
                    upload_key: key,
                    original_filename: request.filename,
                    declared_mime_type: request.declared_mime_type,
                    declared_size_bytes: request.size_bytes,
                    upload_expires_at,
                    uploaded_by: principal.user.id,
                },
                meta,
            )
            .await?;
        Ok(UploadTicket { media, upload })
    }

    #[tracing::instrument(name = "media.list", skip_all)]
    pub async fn list(
        &self,
        principal: &Principal,
        page: &PageRequest,
        filter: MediaFilter,
    ) -> Result<Page<Media>> {
        require_any(principal)?;
        page.validate()?;
        let filter = if can_manage_all(principal) {
            filter
        } else {
            MediaFilter {
                uploaded_by: Some(principal.user.id),
                ..filter
            }
        };
        self.repository.list(page, &filter).await
    }

    #[tracing::instrument(name = "media.get", skip_all)]
    pub async fn get(&self, principal: &Principal, id: Uuid) -> Result<Media> {
        Ok(self.load(principal, id).await?.media)
    }

    /// 确认上传和/或修改替代文本。先确认再修改替代文本；确认失败时不修改替代文本。
    #[tracing::instrument(name = "media.update", skip_all)]
    pub async fn update(
        &self,
        principal: &Principal,
        id: Uuid,
        update: MediaUpdate,
        meta: &RequestMeta,
    ) -> Result<Media> {
        require_any(principal)?;
        let confirm = match update.status.as_deref() {
            None => false,
            Some("uploaded") => true,
            Some(_) => {
                return Err(AppError::invalid(
                    "status",
                    FieldErrorCode::InvalidValue,
                    "status must be uploaded",
                ));
            }
        };
        let alt_text = update
            .alt_text
            .as_deref()
            .map(normalize_alt_text)
            .transpose()?;
        let record = self.load(principal, id).await?;
        let actor = MediaActor {
            user_id: principal.user.id,
            meta,
        };
        let mut media = record.media.clone();
        if confirm {
            media = self.confirm(record, &actor).await?;
        }
        if let Some(alt_text) = alt_text {
            media = self
                .repository
                .update_alt_text(id, alt_text.as_deref(), &actor)
                .await?;
        }
        Ok(media)
    }

    /// 以字节流读取已校验的文件内容，不把整个文件读入内存。
    #[tracing::instrument(name = "media.content", skip_all)]
    pub async fn content(&self, principal: &Principal, id: Uuid) -> Result<MediaContent> {
        let record = self.load(principal, id).await?;
        let (MediaStatus::Ready, Some(key)) = (record.media.status, record.storage_key.as_deref())
        else {
            return Err(AppError::Conflict(Conflict::MediaNotReady));
        };
        match self.storage()?.stream(key, MAX_UPLOAD_BYTES).await? {
            StreamedObject::Stream { size, body } => Ok(MediaContent {
                media: record.media,
                size,
                body,
            }),
            StreamedObject::Missing => Err(AppError::Internal(anyhow::anyhow!(
                "ready media object is missing"
            ))),
            StreamedObject::TooLarge { size } => Err(AppError::Internal(anyhow::anyhow!(
                "ready media object exceeds the size limit: {size} bytes"
            ))),
        }
    }

    /// 删除媒体记录及其对象。记录删除后对象删除失败只记日志，孤立对象不可被引用或读取。
    /// 正式键按 ID 推导后一并删除：待确认记录也可能已有中断的确认写入的正式对象。
    #[tracing::instrument(name = "media.delete", skip_all)]
    pub async fn delete(&self, principal: &Principal, id: Uuid, meta: &RequestMeta) -> Result<()> {
        self.load(principal, id).await?;
        let storage = self.storage()?;
        let deleted = self
            .repository
            .delete(
                id,
                &MediaActor {
                    user_id: principal.user.id,
                    meta,
                },
            )
            .await?;
        discard(storage, &deleted.upload_key).await;
        discard(storage, &storage_key(id)).await;
        Ok(())
    }

    /// 清理过期未确认和保留期已满的已拒绝媒体：先删临时对象，成功后再删记录，
    /// 记录删除成功后再删除可能由中断的确认留下的正式对象；记录若已变为 `ready`
    /// 或仍被引用则不会被删除，其正式对象也不受影响。
    ///
    /// 单条记录失败只记日志并标记为清理失败，继续处理其余记录；失败的记录排到后续批次末尾，
    /// 不会持续挡住其他记录。只有读取待清理列表失败才返回错误。未配置存储时不清理。
    #[tracing::instrument(name = "media.purge_stale", skip_all)]
    pub async fn purge_stale(&self, now: DateTime<Utc>) -> Result<PurgeReport> {
        let Some(storage) = self.storage.as_deref() else {
            return Ok(PurgeReport::default());
        };
        let grace = chrono::Duration::from_std(EXPIRED_UPLOAD_GRACE)
            .map_err(|error| AppError::Internal(error.into()))?;
        let retention = chrono::Duration::from_std(REJECTED_RETENTION)
            .map_err(|error| AppError::Internal(error.into()))?;
        let stale = self
            .repository
            .stale(now - grace, now - retention, PURGE_BATCH)
            .await?;
        let mut report = PurgeReport::default();
        for media in stale {
            let result = match storage.delete(&media.upload_key).await {
                Ok(()) => self.repository.purge(media.id).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(true) => {
                    discard(storage, &storage_key(media.id)).await;
                    report.purged += 1;
                }
                // 状态已变化或仍被引用，记录保留。
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(media_id = %media.id, %error, "stale media cleanup failed");
                    report.failed += 1;
                    if let Err(error) = self.repository.defer_purge(media.id).await {
                        tracing::warn!(media_id = %media.id, %error, "failed to defer stale media cleanup");
                    }
                }
            }
        }
        Ok(report)
    }

    /// 读取调用者可见的媒体；不可见与不存在同样返回 `NotFound`。
    async fn load(&self, principal: &Principal, id: Uuid) -> Result<MediaRecord> {
        require_any(principal)?;
        let record = self.repository.find(id).await?.ok_or(AppError::NotFound)?;
        if can_manage_all(principal) || record.media.uploaded_by == Some(principal.user.id) {
            Ok(record)
        } else {
            Err(AppError::NotFound)
        }
    }

    /// 确认上传。重复确认返回同一结果：已就绪返回记录，已拒绝返回同一拒绝原因。
    async fn confirm(&self, record: MediaRecord, actor: &MediaActor<'_>) -> Result<Media> {
        match record.media.status {
            MediaStatus::Ready => return Ok(record.media),
            MediaStatus::Rejected => return Err(rejected_error(&record.media)),
            MediaStatus::Pending => {}
        }
        if Utc::now() >= record.media.upload_expires_at {
            return Err(AppError::Conflict(Conflict::UploadExpired));
        }
        let storage = self.storage()?;
        let _permit = self
            .confirmations
            .acquire()
            .await
            .map_err(|error| AppError::Internal(error.into()))?;
        let id = record.media.id;
        let formal_key = storage_key(id);
        let bytes = match storage.read(&record.upload_key, MAX_UPLOAD_BYTES).await? {
            StoredObject::Bytes(bytes) => bytes,
            StoredObject::TooLarge { .. } => {
                return self
                    .reject(storage, &record, RejectionReason::TooLarge, actor)
                    .await;
            }
            // 临时对象缺失时，上次确认可能已写入正式对象但未更新记录。
            StoredObject::Missing => match storage.read(&formal_key, MAX_UPLOAD_BYTES).await? {
                StoredObject::Bytes(bytes) => bytes,
                _ => return Err(AppError::Conflict(Conflict::UploadIncomplete)),
            },
        };
        let (bytes, verdict) = self.inspector.inspect(bytes).await?;
        let image = match verdict {
            Ok(image) => image,
            Err(reason) => return self.reject(storage, &record, reason, actor).await,
        };
        let image = match storage
            .put_if_absent(&formal_key, bytes, image.mime_type)
            .await?
        {
            PutOutcome::Created => image,
            // 正式对象不可覆盖，已存在时以其实际内容为准。
            PutOutcome::AlreadyExists => self.verify_existing(storage, &formal_key).await?,
        };
        match self
            .repository
            .mark_ready(id, &formal_key, &image, actor)
            .await?
        {
            Some(media) => {
                discard(storage, &record.upload_key).await;
                Ok(media)
            }
            None => self.settled(storage, id).await,
        }
    }

    async fn verify_existing(
        &self,
        storage: &dyn ObjectStorage,
        key: &str,
    ) -> Result<VerifiedImage> {
        let StoredObject::Bytes(bytes) = storage.read(key, MAX_UPLOAD_BYTES).await? else {
            return Err(AppError::Internal(anyhow::anyhow!(
                "existing formal media object is unreadable"
            )));
        };
        self.inspector.inspect(bytes).await?.1.map_err(|reason| {
            AppError::Internal(anyhow::anyhow!(
                "existing formal media object failed verification: {}",
                reason.as_str()
            ))
        })
    }

    async fn reject(
        &self,
        storage: &dyn ObjectStorage,
        record: &MediaRecord,
        reason: RejectionReason,
        actor: &MediaActor<'_>,
    ) -> Result<Media> {
        match self
            .repository
            .mark_rejected(record.media.id, reason, actor)
            .await?
        {
            Some(_) => {
                discard(storage, &record.upload_key).await;
                Err(reason.into_error())
            }
            None => self.settled(storage, record.media.id).await,
        }
    }

    /// 并发确认已先完成时，按记录的最终状态返回同一结果。记录已被删除时，
    /// 删除本次确认可能刚写入的对象，避免留下无记录引用的正式对象。
    async fn settled(&self, storage: &dyn ObjectStorage, id: Uuid) -> Result<Media> {
        let Some(record) = self.repository.find(id).await? else {
            discard(storage, &upload_key(id)).await;
            discard(storage, &storage_key(id)).await;
            return Err(AppError::NotFound);
        };
        match record.media.status {
            MediaStatus::Ready => Ok(record.media),
            MediaStatus::Rejected => Err(rejected_error(&record.media)),
            MediaStatus::Pending => Err(AppError::Internal(anyhow::anyhow!(
                "media is still pending after a lost confirmation race"
            ))),
        }
    }
}

fn can_manage_all(principal: &Principal) -> bool {
    principal.require(MEDIA_MANAGE).is_ok()
}

fn require_any(principal: &Principal) -> Result<()> {
    if can_manage_all(principal) {
        Ok(())
    } else {
        principal.require(MEDIA_MANAGE_OWN)
    }
}

fn rejected_error(media: &Media) -> AppError {
    media
        .rejection_reason
        .unwrap_or(RejectionReason::UnsupportedFormat)
        .into_error()
}

/// 尽力删除不再需要的对象；失败只记录日志，由存储生命周期规则或 Worker 兜底。
async fn discard(storage: &dyn ObjectStorage, key: &str) {
    if let Err(error) = storage.delete(key).await {
        tracing::warn!(%error, "media object cleanup failed");
    }
}

#[cfg(test)]
mod tests;
