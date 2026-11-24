//! 媒体用例的单元测试，使用内存仓储、内存对象存储和按前缀判定的图片检查器。

use super::*;
use crate::ports::StaleMedia;
use async_trait::async_trait;
use silva_domain::identity::{Principal, User, permissions_for_role};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

#[derive(Default)]
struct MemoryRepository {
    rows: Mutex<HashMap<Uuid, MediaRecord>>,
    audits: Mutex<Vec<&'static str>>,
    /// 与数据库一致：保留期按拒绝时间计算，不随其他更新变化。
    rejected_at: Mutex<HashMap<Uuid, DateTime<Utc>>>,
    /// 模拟确认期间记录被并发删除：标记为 ready 时删除记录并返回 None。
    vanish_on_ready: std::sync::atomic::AtomicBool,
    /// 仍被文章或个人资料引用的媒体，不可清理。
    referenced: Mutex<std::collections::HashSet<Uuid>>,
    /// 最近一次清理失败的时间。
    purge_attempted_at: Mutex<HashMap<Uuid, DateTime<Utc>>>,
}

impl MemoryRepository {
    fn rows(&self) -> MutexGuard<'_, HashMap<Uuid, MediaRecord>> {
        self.rows.lock().unwrap()
    }

    fn audit(&self, action: &'static str) {
        self.audits.lock().unwrap().push(action);
    }

    fn transition(
        &self,
        id: Uuid,
        change: impl FnOnce(&mut MediaRecord),
        action: &'static str,
    ) -> Option<Media> {
        let mut rows = self.rows();
        let record = rows.get_mut(&id)?;
        if record.media.status != MediaStatus::Pending {
            return None;
        }
        change(record);
        record.media.updated_at = Utc::now();
        self.audit(action);
        Some(record.media.clone())
    }
}

#[async_trait]
impl MediaRepository for MemoryRepository {
    async fn create(&self, new: &NewMedia, _meta: &RequestMeta) -> Result<Media> {
        let now = Utc::now();
        let media = Media {
            id: new.id,
            status: MediaStatus::Pending,
            original_filename: new.original_filename.clone(),
            declared_mime_type: new.declared_mime_type.clone(),
            declared_size_bytes: new.declared_size_bytes as i64,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            alt_text: None,
            rejection_reason: None,
            uploaded_by: Some(new.uploaded_by),
            upload_expires_at: new.upload_expires_at,
            created_at: now,
            updated_at: now,
        };
        self.rows().insert(
            new.id,
            MediaRecord {
                media: media.clone(),
                upload_key: new.upload_key.clone(),
                storage_key: None,
            },
        );
        self.audit("media.create");
        Ok(media)
    }

    async fn find(&self, id: Uuid) -> Result<Option<MediaRecord>> {
        Ok(self.rows().get(&id).cloned())
    }

    async fn list(&self, page: &PageRequest, filter: &MediaFilter) -> Result<Page<Media>> {
        let mut items: Vec<_> = self
            .rows()
            .values()
            .filter(|record| filter.status.is_none_or(|s| record.media.status == s))
            .filter(|record| {
                filter
                    .uploaded_by
                    .is_none_or(|owner| record.media.uploaded_by == Some(owner))
            })
            .map(|record| record.media.clone())
            .collect();
        items.sort_by_key(|media| std::cmp::Reverse(media.id));
        let total = items.len() as i64;
        Ok(Page {
            items,
            pagination: page.metadata(total),
        })
    }

    async fn mark_ready(
        &self,
        id: Uuid,
        storage_key: &str,
        image: &VerifiedImage,
        _actor: &MediaActor<'_>,
    ) -> Result<Option<Media>> {
        if self
            .vanish_on_ready
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.rows().remove(&id);
            return Ok(None);
        }
        Ok(self.transition(
            id,
            |record| {
                record.storage_key = Some(storage_key.to_owned());
                record.media.status = MediaStatus::Ready;
                record.media.mime_type = Some(image.mime_type.to_owned());
                record.media.size_bytes = Some(image.size_bytes as i64);
                record.media.width = Some(image.width as i32);
                record.media.height = Some(image.height as i32);
            },
            "media.ready",
        ))
    }

    async fn mark_rejected(
        &self,
        id: Uuid,
        reason: RejectionReason,
        _actor: &MediaActor<'_>,
    ) -> Result<Option<Media>> {
        Ok(self.transition(
            id,
            |record| {
                record.media.status = MediaStatus::Rejected;
                record.media.rejection_reason = Some(reason);
                self.rejected_at.lock().unwrap().insert(id, Utc::now());
            },
            "media.rejected",
        ))
    }

    async fn update_alt_text(
        &self,
        id: Uuid,
        alt_text: Option<&str>,
        _actor: &MediaActor<'_>,
    ) -> Result<Media> {
        let mut rows = self.rows();
        let record = rows.get_mut(&id).ok_or(AppError::NotFound)?;
        record.media.alt_text = alt_text.map(str::to_owned);
        // 与数据库触发器一致：任何更新都刷新 updated_at。
        record.media.updated_at = Utc::now();
        self.audit("media.update");
        Ok(record.media.clone())
    }

    async fn delete(&self, id: Uuid, _actor: &MediaActor<'_>) -> Result<MediaRecord> {
        let record = self.rows().remove(&id).ok_or(AppError::NotFound)?;
        self.audit("media.delete");
        Ok(record)
    }

    async fn stale(
        &self,
        expired_before: DateTime<Utc>,
        rejected_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StaleMedia>> {
        let referenced = self.referenced.lock().unwrap().clone();
        let attempted = self.purge_attempted_at.lock().unwrap().clone();
        let mut stale: Vec<_> = self
            .rows()
            .values()
            .filter(|record| !referenced.contains(&record.media.id))
            .filter(|record| match record.media.status {
                MediaStatus::Pending => record.media.upload_expires_at < expired_before,
                MediaStatus::Rejected => {
                    self.rejected_at.lock().unwrap()[&record.media.id] < rejected_before
                }
                MediaStatus::Ready => false,
            })
            .map(|record| record.media.clone())
            .collect();
        // 与数据库一致：从未失败的在前，失败过的按失败时间排后。
        stale.sort_by_key(|media| {
            (
                attempted.get(&media.id).copied(),
                media.created_at,
                media.id,
            )
        });
        let rows = self.rows();
        Ok(stale
            .into_iter()
            .take(limit as usize)
            .map(|media| StaleMedia {
                id: media.id,
                upload_key: rows[&media.id].upload_key.clone(),
            })
            .collect())
    }

    async fn purge(&self, id: Uuid) -> Result<bool> {
        let mut rows = self.rows();
        let removable = !self.referenced.lock().unwrap().contains(&id)
            && rows
                .get(&id)
                .is_some_and(|record| record.media.status != MediaStatus::Ready);
        if removable {
            rows.remove(&id);
        }
        Ok(removable)
    }

    async fn defer_purge(&self, id: Uuid) -> Result<()> {
        self.purge_attempted_at
            .lock()
            .unwrap()
            .insert(id, Utc::now());
        Ok(())
    }
}

#[derive(Default)]
struct MemoryStorage {
    objects: Mutex<HashMap<String, Vec<u8>>>,
    /// 最近一次签发的直传地址绑定的字节数。
    signed_length: Mutex<Option<u64>>,
    /// 删除时总是失败的键。
    failing_deletes: Mutex<std::collections::HashSet<String>>,
}

impl MemoryStorage {
    fn put(&self, key: &str, bytes: &[u8]) {
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_owned(), bytes.to_vec());
    }

    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.lock().unwrap().get(key).cloned()
    }
}

#[async_trait]
impl ObjectStorage for MemoryStorage {
    async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: u64,
        _expires_in: Duration,
    ) -> Result<PresignedUpload> {
        *self.signed_length.lock().unwrap() = Some(content_length);
        Ok(PresignedUpload {
            method: "PUT".into(),
            url: format!("https://storage.test/{key}?signature=x"),
            headers: vec![("content-type".into(), content_type.into())],
        })
    }

    async fn read(&self, key: &str, max_bytes: u64) -> Result<StoredObject> {
        Ok(match self.get(key) {
            None => StoredObject::Missing,
            Some(bytes) if bytes.len() as u64 > max_bytes => StoredObject::TooLarge {
                size: bytes.len() as u64,
            },
            Some(bytes) => StoredObject::Bytes(bytes),
        })
    }

    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Vec<u8>,
        _content_type: &str,
    ) -> Result<PutOutcome> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(key) {
            return Ok(PutOutcome::AlreadyExists);
        }
        objects.insert(key.to_owned(), bytes);
        Ok(PutOutcome::Created)
    }

    async fn stream(&self, key: &str, max_bytes: u64) -> Result<StreamedObject> {
        Ok(match self.get(key) {
            None => StreamedObject::Missing,
            Some(bytes) if bytes.len() as u64 > max_bytes => StreamedObject::TooLarge {
                size: bytes.len() as u64,
            },
            Some(bytes) => StreamedObject::Stream {
                size: bytes.len() as u64,
                body: Box::pin(futures_util::stream::iter([Ok(bytes::Bytes::from(bytes))])),
            },
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        if self.failing_deletes.lock().unwrap().contains(key) {
            return Err(AppError::Internal(anyhow::anyhow!(
                "simulated delete failure"
            )));
        }
        self.objects.lock().unwrap().remove(key);
        Ok(())
    }
}

/// 以 `PNG:<宽>` 开头的字节视为合法 PNG（高固定为 10），`BIG` 开头视为尺寸超限，其余视为不支持。
struct PrefixInspector;

#[async_trait]
impl ImageInspector for PrefixInspector {
    async fn inspect(
        &self,
        bytes: Vec<u8>,
    ) -> Result<(Vec<u8>, std::result::Result<VerifiedImage, RejectionReason>)> {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let verdict = if let Some(width) = text.strip_prefix("PNG:") {
            Ok(VerifiedImage {
                mime_type: "image/png",
                size_bytes: bytes.len() as u64,
                width: width.trim().parse().unwrap_or(1),
                height: 10,
            })
        } else if text.starts_with("BIG") {
            Err(RejectionReason::DimensionsExceeded)
        } else {
            Err(RejectionReason::UnsupportedFormat)
        };
        Ok((bytes, verdict))
    }
}

struct Fixture {
    service: MediaService,
    repository: Arc<MemoryRepository>,
    storage: Arc<MemoryStorage>,
}

fn fixture() -> Fixture {
    let repository = Arc::new(MemoryRepository::default());
    let storage = Arc::new(MemoryStorage::default());
    let service = MediaService::new(
        repository.clone(),
        Some(storage.clone()),
        Arc::new(PrefixInspector),
        Duration::from_secs(900),
    );
    Fixture {
        service,
        repository,
        storage,
    }
}

fn principal(role: &str) -> Principal {
    let now = Utc::now();
    Principal {
        session_id: ids::new_id(),
        user: User {
            id: ids::new_id(),
            email: format!("{role}@example.test"),
            display_name: role.into(),
            status: "active".into(),
            roles: vec![role.into()],
            created_at: now,
            updated_at: now,
            last_login_at: None,
        },
        permissions: permissions_for_role(role)
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

fn png_request() -> UploadRequest {
    UploadRequest {
        filename: "cover.png".into(),
        declared_mime_type: "image/png".into(),
        size_bytes: 12,
    }
}

fn confirm() -> MediaUpdate {
    MediaUpdate {
        status: Some("uploaded".into()),
        alt_text: None,
    }
}

impl Fixture {
    async fn upload(&self, owner: &Principal) -> UploadTicket {
        self.service
            .create_upload(owner, png_request(), &RequestMeta::default())
            .await
            .unwrap()
    }

    async fn update(&self, who: &Principal, id: Uuid, update: MediaUpdate) -> Result<Media> {
        self.service
            .update(who, id, update, &RequestMeta::default())
            .await
    }

    fn audits(&self) -> Vec<&'static str> {
        self.repository.audits.lock().unwrap().clone()
    }
}

fn assert_media_invalid(result: Result<Media>, expected: RejectionReason) {
    match result {
        Err(AppError::MediaInvalid { reason }) => assert_eq!(reason, expected.as_str()),
        other => panic!(
            "期望 media_invalid（{}），实际为 {other:?}",
            expected.as_str()
        ),
    }
}

#[tokio::test]
async fn create_upload_signs_temporary_key_and_creates_pending_record() {
    let f = fixture();
    let author = principal("author");
    let before = Utc::now();
    let ticket = f.upload(&author).await;
    let id = ticket.media.id;
    assert_eq!(ticket.media.status, MediaStatus::Pending);
    assert_eq!(ticket.media.uploaded_by, Some(author.user.id));
    assert_eq!(ticket.upload.method, "PUT");
    assert!(ticket.upload.url.contains(&format!("uploads/{id}")));
    assert_eq!(
        ticket.upload.headers,
        vec![("content-type".to_owned(), "image/png".to_owned())]
    );
    let ttl = ticket.media.upload_expires_at - before;
    assert!(ttl >= chrono::Duration::seconds(899) && ttl <= chrono::Duration::seconds(901));
    assert_eq!(f.audits(), ["media.create"]);
    assert_eq!(
        *f.storage.signed_length.lock().unwrap(),
        Some(png_request().size_bytes),
        "直传签名绑定声明的字节数"
    );
}

#[tokio::test]
async fn create_upload_validates_input_and_requires_configured_storage() {
    let f = fixture();
    let author = principal("author");
    let invalid = UploadRequest {
        filename: "".into(),
        declared_mime_type: "image/gif".into(),
        size_bytes: 0,
    };
    assert!(matches!(
        f.service
            .create_upload(&author, invalid, &RequestMeta::default())
            .await,
        Err(AppError::Validation(errors)) if errors.len() == 3
    ));
    let unconfigured = MediaService::new(
        f.repository.clone(),
        None,
        Arc::new(PrefixInspector),
        Duration::from_secs(900),
    );
    assert!(matches!(
        unconfigured
            .create_upload(&author, png_request(), &RequestMeta::default())
            .await,
        Err(AppError::StorageUnavailable)
    ));
    assert!(f.repository.rows().is_empty(), "存储不可用时不创建记录");
}

#[tokio::test]
async fn confirming_valid_upload_writes_formal_object_and_is_idempotent() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    f.storage.put(&upload_key(id), b"PNG:64");
    let media = f.update(&author, id, confirm()).await.unwrap();
    assert_eq!(media.status, MediaStatus::Ready);
    assert_eq!(media.mime_type.as_deref(), Some("image/png"));
    assert_eq!((media.width, media.height), (Some(64), Some(10)));
    assert_eq!(media.size_bytes, Some(6));
    assert_eq!(f.storage.get(&storage_key(id)).unwrap(), b"PNG:64");
    assert!(f.storage.get(&upload_key(id)).is_none(), "临时对象已删除");
    // 重复确认返回同一结果，不再写审计。
    let again = f.update(&author, id, confirm()).await.unwrap();
    assert_eq!(again.status, MediaStatus::Ready);
    assert_eq!(f.audits(), ["media.create", "media.ready"]);
    let content = f.service.content(&author, id).await.unwrap();
    assert_eq!(content.size, 6);
    let chunks: Vec<_> = futures_util::StreamExt::collect(content.body).await;
    let bytes: Vec<u8> = chunks
        .into_iter()
        .flat_map(|chunk| chunk.unwrap())
        .collect();
    assert_eq!(bytes, b"PNG:64");
}

#[tokio::test]
async fn confirming_before_upload_or_after_expiry_is_a_conflict() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    assert!(matches!(
        f.update(&author, id, confirm()).await,
        Err(AppError::Conflict(Conflict::UploadIncomplete))
    ));
    assert!(matches!(
        f.service.content(&author, id).await,
        Err(AppError::Conflict(Conflict::MediaNotReady))
    ));
    f.storage.put(&upload_key(id), b"PNG:1");
    f.repository
        .rows()
        .get_mut(&id)
        .unwrap()
        .media
        .upload_expires_at = Utc::now() - chrono::Duration::seconds(1);
    assert!(matches!(
        f.update(&author, id, confirm()).await,
        Err(AppError::Conflict(Conflict::UploadExpired))
    ));
    assert_eq!(
        f.repository.rows()[&id].media.status,
        MediaStatus::Pending,
        "过期确认不改变记录"
    );
}

#[tokio::test]
async fn invalid_uploads_are_rejected_once_and_keep_their_reason() {
    let f = fixture();
    let author = principal("author");
    for (bytes, reason) in [
        (b"GIF89a".to_vec(), RejectionReason::UnsupportedFormat),
        (b"BIG".to_vec(), RejectionReason::DimensionsExceeded),
        (
            vec![b'P'; MAX_UPLOAD_BYTES as usize + 1],
            RejectionReason::TooLarge,
        ),
    ] {
        let id = f.upload(&author).await.media.id;
        f.storage.put(&upload_key(id), &bytes);
        assert_media_invalid(f.update(&author, id, confirm()).await, reason);
        assert!(
            f.storage.get(&upload_key(id)).is_none(),
            "拒绝后删除临时对象"
        );
        assert!(f.storage.get(&storage_key(id)).is_none());
        let record = f.repository.rows()[&id].clone();
        assert_eq!(record.media.status, MediaStatus::Rejected);
        assert_eq!(record.media.rejection_reason, Some(reason));
        // 被拒绝后重复确认返回同一错误，即使再次上传合法内容也不改变结果。
        f.storage.put(&upload_key(id), b"PNG:5");
        assert_media_invalid(f.update(&author, id, confirm()).await, reason);
    }
}

#[tokio::test]
async fn existing_formal_object_wins_after_interrupted_confirmation() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    // 模拟上次确认写入正式对象后中断，随后客户端又上传了不同内容。
    f.storage.put(&storage_key(id), b"PNG:300");
    f.storage.put(&upload_key(id), b"PNG:7");
    let media = f.update(&author, id, confirm()).await.unwrap();
    assert_eq!(media.width, Some(300), "以不可覆盖的正式对象为准");
    assert_eq!(f.storage.get(&storage_key(id)).unwrap(), b"PNG:300");

    // 临时对象已被删除但正式对象存在时同样可以完成确认。
    let other = f.upload(&author).await.media.id;
    f.storage.put(&storage_key(other), b"PNG:9");
    let media = f.update(&author, other, confirm()).await.unwrap();
    assert_eq!(media.width, Some(9));
}

#[tokio::test]
async fn authors_only_see_their_own_media_and_editors_see_all() {
    let f = fixture();
    let author = principal("author");
    let other = principal("author");
    let editor = principal("editor");
    let mine = f.upload(&author).await.media.id;
    let theirs = f.upload(&other).await.media.id;
    assert!(matches!(
        f.service.get(&author, theirs).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        f.update(&author, theirs, confirm()).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        f.service
            .delete(&author, theirs, &RequestMeta::default())
            .await,
        Err(AppError::NotFound)
    ));
    let page = PageRequest::default();
    let listed = f
        .service
        .list(&author, &page, MediaFilter::default())
        .await
        .unwrap();
    assert_eq!(
        listed.items.iter().map(|m| m.id).collect::<Vec<_>>(),
        [mine]
    );
    // 作者不能通过筛选参数查看他人媒体。
    let forced = f
        .service
        .list(
            &author,
            &page,
            MediaFilter {
                uploaded_by: Some(other.user.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        forced.items.iter().map(|m| m.id).collect::<Vec<_>>(),
        [mine]
    );
    let all = f
        .service
        .list(&editor, &page, MediaFilter::default())
        .await
        .unwrap();
    assert_eq!(all.pagination.total, 2);
    assert!(f.service.get(&editor, theirs).await.is_ok());

    let mut nobody = principal("author");
    nobody.permissions.clear();
    assert!(matches!(
        f.service.get(&nobody, mine).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn alt_text_is_normalized_and_status_only_accepts_uploaded() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    let media = f
        .update(
            &author,
            id,
            MediaUpdate {
                status: None,
                alt_text: Some("  封面图 ".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(media.alt_text.as_deref(), Some("封面图"));
    let cleared = f
        .update(
            &author,
            id,
            MediaUpdate {
                status: None,
                alt_text: Some(" ".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.alt_text, None);
    assert!(matches!(
        f.update(
            &author,
            id,
            MediaUpdate {
                status: Some("ready".into()),
                alt_text: None,
            },
        )
        .await,
        Err(AppError::Validation(errors)) if errors[0].field == "status"
    ));
    // 确认失败时不修改替代文本。
    assert!(
        f.update(
            &author,
            id,
            MediaUpdate {
                status: Some("uploaded".into()),
                alt_text: Some("不会写入".into()),
            },
        )
        .await
        .is_err()
    );
    assert_eq!(f.repository.rows()[&id].media.alt_text, None);
}

#[tokio::test]
async fn delete_removes_record_and_objects() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    f.storage.put(&upload_key(id), b"PNG:2");
    f.update(&author, id, confirm()).await.unwrap();
    f.service
        .delete(&author, id, &RequestMeta::default())
        .await
        .unwrap();
    assert!(f.repository.rows().is_empty());
    assert!(f.storage.get(&storage_key(id)).is_none());
    assert!(matches!(
        f.service.get(&author, id).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn purge_removes_expired_pending_and_old_rejected_but_keeps_ready() {
    let f = fixture();
    let author = principal("author");
    let expired = f.upload(&author).await.media.id;
    let fresh = f.upload(&author).await.media.id;
    let ready = f.upload(&author).await.media.id;
    f.storage.put(&upload_key(expired), b"partial");
    f.storage.put(&upload_key(ready), b"PNG:3");
    f.update(&author, ready, confirm()).await.unwrap();
    {
        let mut rows = f.repository.rows();
        rows.get_mut(&expired).unwrap().media.upload_expires_at =
            Utc::now() - chrono::Duration::hours(2);
        rows.get_mut(&ready).unwrap().media.upload_expires_at =
            Utc::now() - chrono::Duration::hours(2);
    }
    let report = f.service.purge_stale(Utc::now()).await.unwrap();
    assert_eq!(
        report,
        PurgeReport {
            purged: 1,
            failed: 0
        }
    );
    let rows = f.repository.rows();
    assert!(!rows.contains_key(&expired));
    assert!(rows.contains_key(&fresh) && rows.contains_key(&ready));
    assert!(f.storage.get(&upload_key(expired)).is_none());
}

#[tokio::test]
async fn purge_and_delete_remove_formal_objects_left_by_interrupted_confirmations() {
    let f = fixture();
    let author = principal("author");
    // 确认写入正式对象后记录未更新（例如数据库超时），随后过期被清理。
    let expired = f.upload(&author).await.media.id;
    f.storage.put(&upload_key(expired), b"PNG:4");
    f.storage.put(&storage_key(expired), b"PNG:4");
    f.repository
        .rows()
        .get_mut(&expired)
        .unwrap()
        .media
        .upload_expires_at = Utc::now() - chrono::Duration::hours(2);
    assert_eq!(f.service.purge_stale(Utc::now()).await.unwrap().purged, 1);
    assert!(f.storage.get(&upload_key(expired)).is_none());
    assert!(f.storage.get(&storage_key(expired)).is_none());

    // 删除仍为 pending 的记录时同样删除可能存在的正式对象。
    let pending = f.upload(&author).await.media.id;
    f.storage.put(&storage_key(pending), b"PNG:4");
    f.service
        .delete(&author, pending, &RequestMeta::default())
        .await
        .unwrap();
    assert!(f.storage.get(&storage_key(pending)).is_none());
}

#[tokio::test]
async fn confirmation_racing_a_delete_removes_the_object_it_wrote() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    f.storage.put(&upload_key(id), b"PNG:6");
    f.repository
        .vanish_on_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        f.update(&author, id, confirm()).await,
        Err(AppError::NotFound)
    ));
    assert!(
        f.storage.get(&storage_key(id)).is_none(),
        "不留下无记录的正式对象"
    );
    assert!(f.storage.get(&upload_key(id)).is_none());
}

#[tokio::test]
async fn editing_alt_text_does_not_postpone_rejected_retention() {
    let f = fixture();
    let author = principal("author");
    let id = f.upload(&author).await.media.id;
    f.storage.put(&upload_key(id), b"GIF89a");
    assert!(f.update(&author, id, confirm()).await.is_err());
    f.repository
        .rejected_at
        .lock()
        .unwrap()
        .insert(id, Utc::now() - chrono::Duration::days(8));
    // 更新替代文本会刷新 updated_at，但不影响按拒绝时间计算的保留期。
    f.update(
        &author,
        id,
        MediaUpdate {
            status: None,
            alt_text: Some("说明".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(f.service.purge_stale(Utc::now()).await.unwrap().purged, 1);
    assert!(f.repository.rows().is_empty());
}

#[tokio::test]
async fn purge_continues_past_failures_defers_them_and_skips_referenced_media() {
    let f = fixture();
    let author = principal("author");
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = f.upload(&author).await.media.id;
        f.repository
            .rows()
            .get_mut(&id)
            .unwrap()
            .media
            .upload_expires_at = Utc::now() - chrono::Duration::hours(2);
        ids.push(id);
    }
    let (failing, referenced, normal) = (ids[0], ids[1], ids[2]);
    f.storage
        .failing_deletes
        .lock()
        .unwrap()
        .insert(upload_key(failing));
    f.repository.referenced.lock().unwrap().insert(referenced);

    // 第一条清理失败不影响后续记录；被引用的记录不清理。
    let report = f.service.purge_stale(Utc::now()).await.unwrap();
    assert_eq!(
        report,
        PurgeReport {
            purged: 1,
            failed: 1
        }
    );
    let rows = f
        .repository
        .rows()
        .keys()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    assert!(rows.contains(&failing) && rows.contains(&referenced) && !rows.contains(&normal));
    assert!(
        f.repository
            .purge_attempted_at
            .lock()
            .unwrap()
            .contains_key(&failing),
        "失败的记录被标记，排到后续批次末尾"
    );

    // 失败的记录排在从未失败的记录之后。
    let later = f.upload(&author).await.media.id;
    f.repository
        .rows()
        .get_mut(&later)
        .unwrap()
        .media
        .upload_expires_at = Utc::now() - chrono::Duration::hours(2);
    let order: Vec<Uuid> = f
        .repository
        .stale(Utc::now(), Utc::now(), 10)
        .await
        .unwrap()
        .into_iter()
        .map(|media| media.id)
        .collect();
    assert_eq!(order, [later, failing]);
}
