//! 共享对象的登记、引用切换和回收。数据库先登记不可重用的对象键，
//! 由短期凭证保护事务外的对象写入，确认与回收均按摘要串行。

use super::{MEDIA_COLUMNS, PgMediaRepository, audit, insert_pending, read_record};
use crate::persistence::{begin, db};
use serde_json::json;
use silva_application::ports::MediaRecord;
use silva_application::ports::{BlobReservation, CheckedUpload, MediaWrite};
use silva_shared::error::{AppError, Result};
use silva_shared::ids;

impl PgMediaRepository {
    pub(super) async fn reserve(&self, digest: &str) -> Result<BlobReservation> {
        let mut tx = begin(&self.pool).await.map_err(db)?;
        // 冲突时锁定现行键，回收和新一轮键分配不能插入登记凭证的间隙。
        let (key, stored): (String, bool) = sqlx::query_as(
            "INSERT INTO media_blobs AS b(sha256,storage_key) VALUES ($1,$2)
             ON CONFLICT (sha256) DO UPDATE SET stored=b.stored RETURNING storage_key,stored",
        )
        .bind(digest)
        .bind(format!("blobs/{}", ids::new_id()))
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        let id = ids::new_id();
        sqlx::query("INSERT INTO media_blob_writes(id,sha256) VALUES ($1,$2)")
            .bind(id)
            .bind(digest)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(BlobReservation { id, key, stored })
    }

    pub(super) async fn store_checked(
        &self,
        file: CheckedUpload,
        mode: MediaWrite<'_>,
        reservation: BlobReservation,
    ) -> Result<Option<MediaRecord>> {
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let key = reservation.key;
        let stored: Option<bool> = sqlx::query_scalar(
            "SELECT stored FROM media_blobs WHERE sha256=$1 AND storage_key=$2 FOR UPDATE",
        )
        .bind(&file.digest)
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let consumed = sqlx::query("DELETE FROM media_blob_writes WHERE id=$1 AND sha256=$2")
            .bind(reservation.id)
            .bind(&file.digest)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let Some(stored) = stored.filter(|_| consumed.rows_affected() == 1) else {
            // 长时间暂停的调用可能在旧键退役后才返回，再次登记旧键以回收迟到的写入。
            sqlx::query(
                "INSERT INTO media_object_deletions(storage_key)
                SELECT $1 WHERE NOT EXISTS(SELECT FROM media_blobs WHERE storage_key=$1)
                ON CONFLICT DO NOTHING",
            )
            .bind(&key)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            tx.commit().await.map_err(db)?;
            return Err(AppError::Internal(anyhow::anyhow!(
                "media write reservation expired"
            )));
        };
        // 新上传只在确实需要媒体条目时创建记录和审计。旧 ID 在回填和确认时保留。
        let mut legacy_key = None;
        match mode {
            MediaWrite::Upload(new, actor) => {
                let existing = sqlx::query(&format!(
                    "SELECT {MEDIA_COLUMNS} FROM media_assets WHERE uploaded_by=$1 AND blob_sha256=$2
                     ORDER BY id LIMIT 1 FOR KEY SHARE"
                ))
                .bind(new.uploaded_by)
                .bind(&file.digest)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
                if let Some(row) = existing {
                    tx.commit().await.map_err(db)?;
                    return Ok(Some(read_record(&row).map_err(db)?));
                }
                insert_pending(&mut tx, new, actor.meta).await?;
            }
            MediaWrite::Confirm(_) | MediaWrite::Backfill => {
                let expected = if matches!(mode, MediaWrite::Backfill) {
                    "ready"
                } else {
                    "pending"
                };
                let row: Option<(Option<String>,)> = sqlx::query_as(
                    "SELECT storage_key FROM media_assets WHERE id=$1 AND status=$2 AND blob_sha256 IS NULL
                     FOR UPDATE",
                )
                .bind(file.id)
                .bind(expected)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
                let Some((old_key,)) = row else {
                    tx.commit().await.map_err(db)?;
                    return Ok(None);
                };
                legacy_key = old_key;
            }
        }
        if !stored {
            sqlx::query("UPDATE media_blobs SET stored=true WHERE sha256=$1")
                .bind(&file.digest)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        let image = file.image;
        let row = sqlx::query(&format!(
            "UPDATE media_assets SET status='ready',blob_sha256=$2,storage_key=$3,
                    mime_type=$4,size_bytes=$5,width=$6,height=$7
             WHERE id=$1 RETURNING {MEDIA_COLUMNS}"
        ))
        .bind(file.id)
        .bind(&file.digest)
        .bind(key)
        .bind(image.mime_type)
        .bind(i64::try_from(image.size_bytes).map_err(|e| AppError::Internal(e.into()))?)
        .bind(i32::try_from(image.width).map_err(|e| AppError::Internal(e.into()))?)
        .bind(i32::try_from(image.height).map_err(|e| AppError::Internal(e.into()))?)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if matches!(mode, MediaWrite::Confirm(_)) {
            // 确认提交后进程可能在删除旧对象前中断，两种私有键都登记重试。
            sqlx::query(
                "INSERT INTO media_object_deletions(storage_key)
                 SELECT upload_key FROM media_assets WHERE id=$1 UNION ALL SELECT $2::text
                 ON CONFLICT DO NOTHING",
            )
            .bind(file.id)
            .bind(silva_domain::media::storage_key(file.id))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        match mode {
            MediaWrite::Backfill => {
                // 已签发的旧地址最长有效一天。对象指针和延迟删除任务同时提交。
                sqlx::query(
                    "INSERT INTO media_object_deletions(storage_key,retry_at)
                     VALUES ($1,now()+interval '1 day') ON CONFLICT DO NOTHING",
                )
                .bind(legacy_key)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            }
            MediaWrite::Upload(_, actor) | MediaWrite::Confirm(actor) => {
                audit(
                    &mut tx,
                    actor,
                    "media.ready",
                    file.id,
                    json!({
                        "mime_type": image.mime_type, "size_bytes": image.size_bytes,
                        "width": image.width, "height": image.height
                    }),
                )
                .await?;
            }
        }
        tx.commit().await.map_err(db)?;
        Ok(Some(read_record(&row).map_err(db)?))
    }

    pub(super) async fn reclaim(&self, limit: i64) -> Result<Vec<String>> {
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("DELETE FROM media_blob_writes WHERE expires_at <= now()")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let candidates: Vec<String> = sqlx::query_scalar(
            "SELECT sha256 FROM media_blobs b WHERE (stored OR created_at < now()-interval '1 hour')
             AND NOT EXISTS (SELECT FROM media_blob_writes w WHERE w.sha256=b.sha256)
             AND NOT EXISTS (SELECT FROM media_assets m WHERE m.blob_sha256=b.sha256)
             ORDER BY created_at LIMIT $1 FOR UPDATE SKIP LOCKED",
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        // 加锁后在新快照内复查；新上传只能在锁释放后分配新的对象键。
        sqlx::query(
            "WITH retired AS (
             DELETE FROM media_blobs b WHERE sha256=ANY($1)
             AND NOT EXISTS (SELECT FROM media_blob_writes w WHERE w.sha256=b.sha256)
             AND NOT EXISTS (SELECT FROM media_assets m WHERE m.blob_sha256=b.sha256)
             RETURNING storage_key)
             INSERT INTO media_object_deletions(storage_key) SELECT storage_key FROM retired",
        )
        .bind(candidates)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;

        sqlx::query_scalar(
            "SELECT storage_key FROM media_object_deletions WHERE retry_at <= now()
             ORDER BY retry_at,storage_key LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db)
    }
}
