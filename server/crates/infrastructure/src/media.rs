//! 媒体记录的 PostgreSQL 仓储。状态变更与审计在同一事务内提交；
//! 对象存储读写不在数据库事务内进行。

use crate::persistence::{acquire, audit_resource, begin, db};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use silva_application::ports::{
    MediaActor, MediaFilter, MediaRecord, MediaRepository, NewMedia, RequestMeta, StaleMedia,
};
use silva_domain::media::{Media, MediaStatus, RejectionReason, VerifiedImage};
use silva_shared::{
    error::{AppError, Conflict, Result},
    metrics::METRICS,
    pagination::{Page, PageRequest},
};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow};
use uuid::Uuid;

const MEDIA_COLUMNS: &str = "id,status,original_filename,declared_mime_type,declared_size_bytes,mime_type,size_bytes,width,height,alt_text,rejection_reason,uploaded_by,upload_expires_at,created_at,updated_at,upload_key,storage_key";

/// 媒体 `m` 未被任何文章修订、修订正文或个人资料引用；新增引用媒体的表时同步扩充。
const UNREFERENCED: &str = "NOT EXISTS (SELECT 1 FROM post_revisions r WHERE r.cover_media_id=m.id) \
     AND NOT EXISTS (SELECT 1 FROM post_revision_media rm WHERE rm.media_id=m.id) \
     AND NOT EXISTS (SELECT 1 FROM profiles p WHERE p.avatar_media_id=m.id) \
     AND NOT EXISTS (SELECT 1 FROM site_state s WHERE s.active_avatar_media_id=m.id)";

/// 媒体列表筛选；`$1` 状态、`$2` 上传者，未提供时绑定 NULL。
const MEDIA_FILTER: &str =
    "WHERE ($1::text IS NULL OR status=$1) AND ($2::uuid IS NULL OR uploaded_by=$2)";

#[derive(Clone)]
pub struct PgMediaRepository {
    pool: PgPool,
}

impl PgMediaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn decode_error(column: &str, value: &str) -> sqlx::Error {
    sqlx::Error::Decode(format!("unexpected media_assets.{column} value: {value}").into())
}

fn read_record(row: &PgRow) -> std::result::Result<MediaRecord, sqlx::Error> {
    let status: String = row.try_get("status")?;
    let reason: Option<String> = row.try_get("rejection_reason")?;
    let rejection_reason = reason
        .map(|value| {
            RejectionReason::parse(&value).ok_or_else(|| decode_error("rejection_reason", &value))
        })
        .transpose()?;
    Ok(MediaRecord {
        media: Media {
            id: row.try_get("id")?,
            status: MediaStatus::parse(&status).ok_or_else(|| decode_error("status", &status))?,
            original_filename: row.try_get("original_filename")?,
            declared_mime_type: row.try_get("declared_mime_type")?,
            declared_size_bytes: row.try_get("declared_size_bytes")?,
            mime_type: row.try_get("mime_type")?,
            size_bytes: row.try_get("size_bytes")?,
            width: row.try_get("width")?,
            height: row.try_get("height")?,
            alt_text: row.try_get("alt_text")?,
            rejection_reason,
            uploaded_by: row.try_get("uploaded_by")?,
            upload_expires_at: row.try_get("upload_expires_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        },
        upload_key: row.try_get("upload_key")?,
        storage_key: row.try_get("storage_key")?,
    })
}

fn media(row: &PgRow) -> Result<Media> {
    read_record(row).map(|record| record.media).map_err(db)
}

async fn audit(
    connection: &mut PgConnection,
    actor: &MediaActor<'_>,
    action: &str,
    id: Uuid,
    metadata: serde_json::Value,
) -> Result<()> {
    audit_resource(
        connection,
        Some(actor.user_id),
        action,
        "media",
        id,
        actor.meta,
        metadata,
    )
    .await
}

#[async_trait]
impl MediaRepository for PgMediaRepository {
    #[tracing::instrument(name = "db.media_create", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn create(&self, new: &NewMedia, meta: &RequestMeta) -> Result<Media> {
        let _timer = METRICS.db_operation("media_create");
        let size = i64::try_from(new.declared_size_bytes)
            .map_err(|error| AppError::Internal(error.into()))?;
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let row = sqlx::query(&format!(
            "INSERT INTO media_assets (id,upload_key,original_filename,declared_mime_type,declared_size_bytes,upload_expires_at,uploaded_by) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {MEDIA_COLUMNS}"
        ))
        .bind(new.id)
        .bind(&new.upload_key)
        .bind(&new.original_filename)
        .bind(&new.declared_mime_type)
        .bind(size)
        .bind(new.upload_expires_at)
        .bind(new.uploaded_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        audit_resource(
            &mut tx,
            Some(new.uploaded_by),
            "media.create",
            "media",
            new.id,
            meta,
            json!({"declared_mime_type": new.declared_mime_type, "declared_size_bytes": size}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        media(&row)
    }

    #[tracing::instrument(name = "db.media_find", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn find(&self, id: Uuid) -> Result<Option<MediaRecord>> {
        let _timer = METRICS.db_operation("media_find");
        let row = sqlx::query(&format!(
            "SELECT {MEDIA_COLUMNS} FROM media_assets WHERE id=$1"
        ))
        .bind(id)
        .fetch_optional(&mut *acquire(&self.pool).await.map_err(db)?)
        .await
        .map_err(db)?;
        row.as_ref().map(read_record).transpose().map_err(db)
    }

    #[tracing::instrument(name = "db.media_list", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn list(&self, page: &PageRequest, filter: &MediaFilter) -> Result<Page<Media>> {
        let _timer = METRICS.db_operation("media_list");
        let status = filter.status.map(MediaStatus::as_str);
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let total =
            sqlx::query_scalar(&format!("SELECT count(*) FROM media_assets {MEDIA_FILTER}"))
                .bind(status)
                .bind(filter.uploaded_by)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        let rows = sqlx::query(&format!(
            "SELECT {MEDIA_COLUMNS} FROM media_assets {MEDIA_FILTER} ORDER BY created_at DESC,id DESC LIMIT $3 OFFSET $4"
        ))
        .bind(status)
        .bind(filter.uploaded_by)
        .bind(i64::from(page.per_page))
        .bind(page.offset())
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        let items = rows.iter().map(media).collect::<Result<Vec<_>>>()?;
        tx.commit().await.map_err(db)?;
        Ok(Page {
            items,
            pagination: page.metadata(total),
        })
    }

    #[tracing::instrument(name = "db.media_mark_ready", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn mark_ready(
        &self,
        id: Uuid,
        storage_key: &str,
        image: &VerifiedImage,
        actor: &MediaActor<'_>,
    ) -> Result<Option<Media>> {
        let _timer = METRICS.db_operation("media_mark_ready");
        let size =
            i64::try_from(image.size_bytes).map_err(|error| AppError::Internal(error.into()))?;
        let width = i32::try_from(image.width).map_err(|error| AppError::Internal(error.into()))?;
        let height =
            i32::try_from(image.height).map_err(|error| AppError::Internal(error.into()))?;
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let row = sqlx::query(&format!(
            "UPDATE media_assets SET status='ready',storage_key=$2,mime_type=$3,size_bytes=$4,width=$5,height=$6 \
             WHERE id=$1 AND status='pending' RETURNING {MEDIA_COLUMNS}"
        ))
        .bind(id)
        .bind(storage_key)
        .bind(image.mime_type)
        .bind(size)
        .bind(width)
        .bind(height)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let Some(row) = row else {
            tx.commit().await.map_err(db)?;
            return Ok(None);
        };
        audit(
            &mut tx,
            actor,
            "media.ready",
            id,
            json!({"mime_type": image.mime_type, "size_bytes": size, "width": width, "height": height}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        media(&row).map(Some)
    }

    #[tracing::instrument(name = "db.media_mark_rejected", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn mark_rejected(
        &self,
        id: Uuid,
        reason: RejectionReason,
        actor: &MediaActor<'_>,
    ) -> Result<Option<Media>> {
        let _timer = METRICS.db_operation("media_mark_rejected");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let row = sqlx::query(&format!(
            "UPDATE media_assets SET status='rejected',rejection_reason=$2,rejected_at=now() WHERE id=$1 AND status='pending' RETURNING {MEDIA_COLUMNS}"
        ))
        .bind(id)
        .bind(reason.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let Some(row) = row else {
            tx.commit().await.map_err(db)?;
            return Ok(None);
        };
        audit(
            &mut tx,
            actor,
            "media.rejected",
            id,
            json!({"reason": reason.as_str()}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        media(&row).map(Some)
    }

    #[tracing::instrument(name = "db.media_update", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn update_alt_text(
        &self,
        id: Uuid,
        alt_text: Option<&str>,
        actor: &MediaActor<'_>,
    ) -> Result<Media> {
        let _timer = METRICS.db_operation("media_update");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let row = sqlx::query(&format!(
            "UPDATE media_assets SET alt_text=$2 WHERE id=$1 RETURNING {MEDIA_COLUMNS}"
        ))
        .bind(id)
        .bind(alt_text)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(AppError::NotFound)?;
        audit(
            &mut tx,
            actor,
            "media.update",
            id,
            json!({"fields": ["alt_text"]}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        media(&row)
    }

    #[tracing::instrument(name = "db.media_delete", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn delete(&self, id: Uuid, actor: &MediaActor<'_>) -> Result<MediaRecord> {
        let _timer = METRICS.db_operation("media_delete");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        // 行锁与引用方插入时的 FOR KEY SHARE 互斥：检查期间不会出现新的引用。
        let row = sqlx::query(&format!(
            "SELECT {MEDIA_COLUMNS} FROM media_assets WHERE id=$1 FOR UPDATE"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(AppError::NotFound)?;
        let record = read_record(&row).map_err(db)?;
        let unreferenced: bool = sqlx::query_scalar(&format!(
            "SELECT {UNREFERENCED} FROM media_assets m WHERE m.id=$1"
        ))
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if !unreferenced {
            return Err(AppError::Conflict(Conflict::MediaInUse));
        }
        sqlx::query("DELETE FROM media_assets WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|error| match &error {
                // 兜底：未在上方列出的引用表通过外键拒绝删除。
                sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
                    AppError::Conflict(Conflict::MediaInUse)
                }
                _ => db(error),
            })?;
        audit(
            &mut tx,
            actor,
            "media.delete",
            id,
            json!({"status": record.media.status.as_str()}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(record)
    }

    #[tracing::instrument(name = "db.media_stale", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn stale(
        &self,
        expired_before: DateTime<Utc>,
        rejected_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StaleMedia>> {
        let _timer = METRICS.db_operation("media_stale");
        // 从未失败的记录在前，失败过的按最近失败时间排后，持续失败的记录不会挡住其他记录。
        let rows = sqlx::query(&format!(
            "SELECT m.id,m.upload_key FROM media_assets m \
             WHERE ((m.status='pending' AND m.upload_expires_at < $1) OR (m.status='rejected' AND m.rejected_at < $2)) \
             AND {UNREFERENCED} \
             ORDER BY m.purge_attempted_at NULLS FIRST,m.created_at,m.id LIMIT $3"
        ))
        .bind(expired_before)
        .bind(rejected_before)
        .bind(limit)
        .fetch_all(&mut *acquire(&self.pool).await.map_err(db)?)
        .await
        .map_err(db)?;
        rows.iter()
            .map(|row| {
                Ok(StaleMedia {
                    id: row.try_get("id")?,
                    upload_key: row.try_get("upload_key")?,
                })
            })
            .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
            .map_err(db)
    }

    #[tracing::instrument(name = "db.media_purge", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn purge(&self, id: Uuid) -> Result<bool> {
        let _timer = METRICS.db_operation("media_purge");
        let result = sqlx::query(&format!(
            "DELETE FROM media_assets m WHERE m.id=$1 AND m.status IN ('pending','rejected') AND {UNREFERENCED}"
        ))
        .bind(id)
        .execute(&mut *acquire(&self.pool).await.map_err(db)?)
        .await;
        match result {
            Ok(result) => Ok(result.rows_affected() == 1),
            // 检查后才出现的引用由外键拒绝，记录保留。
            Err(sqlx::Error::Database(database)) if database.is_foreign_key_violation() => {
                Ok(false)
            }
            Err(error) => Err(db(error)),
        }
    }

    #[tracing::instrument(name = "db.media_defer_purge", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn defer_purge(&self, id: Uuid) -> Result<()> {
        let _timer = METRICS.db_operation("media_defer_purge");
        sqlx::query("UPDATE media_assets SET purge_attempted_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *acquire(&self.pool).await.map_err(db)?)
            .await
            .map_err(db)?;
        Ok(())
    }
}
