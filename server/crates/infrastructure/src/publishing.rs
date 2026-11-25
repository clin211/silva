//! 站点发布的 PostgreSQL 仓储：登记站点目标、领取与续租构建任务、导出快照数据和原子切换线上版本。
//!
//! `site_state` 单例行是串行化点：登记目标、领取任务、切换和失败处理都先锁定它，
//! 保证 `desired_generation` 只增不减，旧构建不能覆盖新目标。

use crate::persistence::{acquire, audit_resource, begin, db};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use silva_application::ports::{
    Activation, ActiveSite, ClaimedJob, ContentActor, ExportedSite, PublishingRepository,
};
use silva_domain::{
    content::{Heading, UserRef},
    site::{
        PublishJob, PublishJobFilter, PublishJobStatus, SiteTarget, SnapshotCategory,
        SnapshotImage, SnapshotMedia, SnapshotPost, SnapshotTag,
    },
};
use silva_shared::{
    error::{AppError, Conflict, Result},
    ids,
    metrics::METRICS,
    pagination::{Page, PageRequest},
};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow, types::Json};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct PgPublishingRepository {
    pool: PgPool,
}

impl PgPublishingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 在调用方事务内登记新的站点目标：锁定站点状态，分配下一个 generation，创建排队任务与
/// Outbox 事件，并把仍在排队的旧任务合并为 superseded。
pub(crate) async fn enqueue_site_publish(
    connection: &mut PgConnection,
    trigger_type: &str,
    requested_by: Option<Uuid>,
) -> Result<SiteTarget> {
    sqlx::query("SELECT singleton FROM site_state WHERE singleton FOR UPDATE")
        .execute(&mut *connection)
        .await
        .map_err(db)?;
    let generation: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(generation), 0) + 1 FROM publish_jobs")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
    sqlx::query(
        "UPDATE publish_jobs SET status='superseded', finished_at=now() WHERE status='queued'",
    )
    .execute(&mut *connection)
    .await
    .map_err(db)?;
    let job_id = ids::new_id();
    sqlx::query(
        "INSERT INTO publish_jobs (id, generation, status, trigger_type, requested_by) VALUES ($1,$2,'queued',$3,$4)",
    )
    .bind(job_id)
    .bind(generation)
    .bind(trigger_type)
    .bind(requested_by)
    .execute(&mut *connection)
    .await
    .map_err(db)?;
    sqlx::query(
        "INSERT INTO outbox_events (id, aggregate_type, aggregate_id, event_type, payload) VALUES ($1,'site',$2,'site.publish_requested',$3)",
    )
    .bind(ids::new_id())
    .bind(job_id)
    .bind(Json(json!({"generation": generation, "publish_job_id": job_id})))
    .execute(&mut *connection)
    .await
    .map_err(db)?;
    sqlx::query("UPDATE site_state SET desired_generation=$1 WHERE singleton")
        .bind(generation)
        .execute(&mut *connection)
        .await
        .map_err(db)?;
    METRICS.queue.with_label_values(&["publish"]).inc();
    Ok(SiteTarget {
        publish_job_id: job_id,
        desired_generation: generation,
    })
}

const JOB_SELECT: &str = "SELECT j.id,j.generation,j.status,j.trigger_type,j.requested_by,u.display_name AS requested_by_name,j.attempts,j.error_message,j.started_at,j.finished_at,j.created_at FROM publish_jobs j LEFT JOIN users u ON u.id=j.requested_by";

fn read_job(row: &PgRow) -> std::result::Result<PublishJob, sqlx::Error> {
    let status: String = row.try_get("status")?;
    let requested_by: Option<Uuid> = row.try_get("requested_by")?;
    Ok(PublishJob {
        id: row.try_get("id")?,
        generation: row.try_get("generation")?,
        status: PublishJobStatus::parse(&status).ok_or_else(|| {
            sqlx::Error::Decode(format!("unexpected publish_jobs.status value: {status}").into())
        })?,
        trigger_type: row.try_get("trigger_type")?,
        requested_by: match requested_by {
            Some(id) => Some(UserRef {
                id,
                display_name: row.try_get("requested_by_name")?,
            }),
            None => None,
        },
        attempts: row.try_get("attempts")?,
        error_message: row.try_get("error_message")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn job(connection: &mut PgConnection, id: Uuid) -> Result<Option<PublishJob>> {
    sqlx::query(&format!("{JOB_SELECT} WHERE j.id=$1"))
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(db)?
        .as_ref()
        .map(read_job)
        .transpose()
        .map_err(db)
}

/// 锁定站点状态与任务行，返回 (期望 generation, 租约是否仍属于该任务)。
async fn lock_job(
    connection: &mut PgConnection,
    claimed: &ClaimedJob,
) -> Result<(Option<i64>, bool)> {
    let desired: Option<i64> =
        sqlx::query_scalar("SELECT desired_generation FROM site_state WHERE singleton FOR UPDATE")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
    let owned: Option<bool> = sqlx::query_scalar(
        "SELECT status='building' AND lease_token=$2 AND lease_expires_at > now() FROM publish_jobs WHERE id=$1 FOR UPDATE",
    )
    .bind(claimed.id)
    .bind(claimed.lease_token)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;
    Ok((desired, owned.unwrap_or(false)))
}

async fn finish(
    connection: &mut PgConnection,
    id: Uuid,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE publish_jobs SET status=$2, lease_token=NULL, lease_expires_at=NULL, finished_at=now(), error_message=COALESCE($3, error_message) WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(error)
    .execute(connection)
    .await
    .map_err(db)?;
    Ok(())
}

fn lease_interval(lease: Duration) -> String {
    format!("{} milliseconds", lease.as_millis())
}

#[async_trait]
impl PublishingRepository for PgPublishingRepository {
    #[tracing::instrument(name = "db.publish_jobs", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn jobs(
        &self,
        page: &PageRequest,
        filter: &PublishJobFilter,
    ) -> Result<Page<PublishJob>> {
        let _timer = METRICS.db_operation("publish_jobs");
        const FILTER: &str = "WHERE ($1::text IS NULL OR j.status=$1) AND ($2::bigint IS NULL OR j.generation=$2) AND ($3::uuid IS NULL OR j.requested_by=$3)";
        let status = filter.status.map(PublishJobStatus::as_str);
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let total = sqlx::query_scalar(&format!("SELECT count(*) FROM publish_jobs j {FILTER}"))
            .bind(status)
            .bind(filter.generation)
            .bind(filter.requested_by)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let rows = sqlx::query(&format!(
            "{JOB_SELECT} {FILTER} ORDER BY j.generation DESC LIMIT $4 OFFSET $5"
        ))
        .bind(status)
        .bind(filter.generation)
        .bind(filter.requested_by)
        .bind(i64::from(page.per_page))
        .bind(page.offset())
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        let items = rows
            .iter()
            .map(read_job)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Page {
            items,
            pagination: page.metadata(total),
        })
    }

    #[tracing::instrument(name = "db.publish_job", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn job(&self, id: Uuid) -> Result<Option<PublishJob>> {
        let _timer = METRICS.db_operation("publish_job");
        job(&mut *acquire(&self.pool).await.map_err(db)?, id).await
    }

    #[tracing::instrument(name = "db.publish_request", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn request_build(
        &self,
        trigger_type: &str,
        actor: Option<&ContentActor<'_>>,
    ) -> Result<SiteTarget> {
        let _timer = METRICS.db_operation("publish_request");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let target = enqueue_site_publish(&mut tx, trigger_type, actor.map(|a| a.user_id)).await?;
        audit_resource(
            &mut tx,
            actor.map(|a| a.user_id),
            "publishing.request",
            "publish_job",
            target.publish_job_id,
            actor.map(|a| a.meta).unwrap_or(&Default::default()),
            json!({"generation": target.desired_generation, "trigger_type": trigger_type}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(target)
    }

    #[tracing::instrument(name = "db.publish_retry", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn retry(&self, id: Uuid, actor: &ContentActor<'_>) -> Result<PublishJob> {
        let _timer = METRICS.db_operation("publish_retry");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let desired: Option<i64> = sqlx::query_scalar(
            "SELECT desired_generation FROM site_state WHERE singleton FOR UPDATE",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        let row: Option<(i64, String)> =
            sqlx::query_as("SELECT generation, status FROM publish_jobs WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        let (generation, status) = row.ok_or(AppError::NotFound)?;
        if status != "failed" || Some(generation) != desired {
            return Err(AppError::Conflict(Conflict::PublishJobNotRetryable));
        }
        // attempts 保持单调递增：产物位置按尝试序号隔离，重试不能复用旧序号。
        sqlx::query(
            "UPDATE publish_jobs SET status='queued', finished_at=NULL, retry_after=NULL WHERE id=$1",
        )
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit_resource(
            &mut tx,
            Some(actor.user_id),
            "publishing.retry",
            "publish_job",
            id,
            actor.meta,
            json!({"generation": generation}),
        )
        .await?;
        let job = job(&mut tx, id).await?.ok_or(AppError::NotFound)?;
        tx.commit().await.map_err(db)?;
        Ok(job)
    }

    #[tracing::instrument(name = "db.publish_claim", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn claim(&self, lease: Duration, max_attempts: i32) -> Result<Option<ClaimedJob>> {
        let _timer = METRICS.db_operation("publish_claim");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let desired: Option<i64> = sqlx::query_scalar(
            "SELECT desired_generation FROM site_state WHERE singleton FOR UPDATE",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        // Outbox 只用于唤醒；任务表是持久事实来源，领取时一并确认通知。
        sqlx::query(
            "UPDATE outbox_events SET processed_at=now(), attempts=attempts+1 WHERE processed_at IS NULL AND event_type='site.publish_requested'",
        )
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let Some(desired) = desired else {
            tx.commit().await.map_err(db)?;
            return Ok(None);
        };
        // 落后的未上线任务（排队中或租约已过期的构建）直接合并为 superseded。
        sqlx::query(
            "UPDATE publish_jobs SET status='superseded', lease_token=NULL, lease_expires_at=NULL, finished_at=now() \
             WHERE generation < $1 AND (status='queued' OR (status='building' AND lease_expires_at <= now()))",
        )
        .bind(desired)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        // 租约过期且已用尽重试的最新任务标记为 failed，等待管理员重试。
        sqlx::query(
            "UPDATE publish_jobs SET status='failed', lease_token=NULL, lease_expires_at=NULL, finished_at=now(), \
             error_message=COALESCE(error_message, 'build lease expired') \
             WHERE generation=$1 AND status='building' AND lease_expires_at <= now() AND attempts >= $2",
        )
        .bind(desired)
        .bind(max_attempts)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        let token = ids::new_id();
        let claimed: Option<(Uuid, i32)> = sqlx::query_as(
            "UPDATE publish_jobs SET status='building', attempts=attempts+1, lease_token=$2, \
             lease_expires_at=now() + $3::interval, started_at=now(), finished_at=NULL, retry_after=NULL \
             WHERE generation=$1 AND ((status='queued' AND (retry_after IS NULL OR retry_after <= now())) \
               OR (status='building' AND lease_expires_at <= now())) \
             RETURNING id, attempts",
        )
        .bind(desired)
        .bind(token)
        .bind(lease_interval(lease))
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(claimed.map(|(id, attempt)| ClaimedJob {
            id,
            generation: desired,
            attempt,
            lease_token: token,
        }))
    }

    #[tracing::instrument(name = "db.publish_renew", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn renew(&self, job: &ClaimedJob, lease: Duration) -> Result<bool> {
        let _timer = METRICS.db_operation("publish_renew");
        let result = sqlx::query(
            "UPDATE publish_jobs SET lease_expires_at=now() + $3::interval \
             WHERE id=$1 AND lease_token=$2 AND status='building' AND lease_expires_at > now()",
        )
        .bind(job.id)
        .bind(job.lease_token)
        .bind(lease_interval(lease))
        .execute(&mut *acquire(&self.pool).await.map_err(db)?)
        .await
        .map_err(db)?;
        Ok(result.rows_affected() == 1)
    }

    #[tracing::instrument(name = "db.publish_export", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn export(&self, job: &ClaimedJob) -> Result<Option<ExportedSite>> {
        let _timer = METRICS.db_operation("publish_export");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let desired: Option<i64> =
            sqlx::query_scalar("SELECT desired_generation FROM site_state WHERE singleton")
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if desired != Some(job.generation) {
            tx.commit().await.map_err(db)?;
            return Ok(None);
        }
        let posts = sqlx::query(
            "SELECT p.id, r.id AS revision_id, p.slug, r.title, r.excerpt, r.rendered_html, r.rendered_headings, \
               r.is_featured, r.seo_title, r.seo_description, p.first_published_at, p.desired_published_at, \
               m.id AS cover_id, m.mime_type AS cover_mime, m.width AS cover_width, m.height AS cover_height, m.alt_text AS cover_alt, \
               ARRAY(SELECT c.slug FROM post_revision_categories rc JOIN categories c ON c.id=rc.category_id \
                     WHERE rc.revision_id=r.id ORDER BY c.name, c.slug) AS categories, \
               ARRAY(SELECT t.slug FROM post_revision_tags rt JOIN tags t ON t.id=rt.tag_id \
                     WHERE rt.revision_id=r.id ORDER BY t.name, t.slug) AS tags \
             FROM posts p JOIN post_revisions r ON r.id=p.desired_published_revision_id \
             LEFT JOIN media_assets m ON m.id=r.cover_media_id AND m.status='ready' \
             ORDER BY p.first_published_at DESC, p.id",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?
        .iter()
        .map(|row| {
            let html: Option<String> = row.try_get("rendered_html")?;
            let Json(headings): Json<Vec<Heading>> = row.try_get("rendered_headings")?;
            let first_published: Option<DateTime<Utc>> = row.try_get("first_published_at")?;
            let desired_at: Option<DateTime<Utc>> = row.try_get("desired_published_at")?;
            let cover_id: Option<Uuid> = row.try_get("cover_id")?;
            let published_at = first_published.or(desired_at).unwrap_or_else(Utc::now);
            Ok(SnapshotPost {
                id: row.try_get("id")?,
                revision_id: row.try_get("revision_id")?,
                slug: row.try_get("slug")?,
                title: row.try_get("title")?,
                excerpt: row.try_get("excerpt")?,
                html: html.unwrap_or_default(),
                headings,
                categories: row.try_get("categories")?,
                tags: row.try_get("tags")?,
                featured: row.try_get("is_featured")?,
                cover: match cover_id {
                    Some(media_id) => Some(SnapshotImage {
                        media_id,
                        mime_type: row.try_get("cover_mime")?,
                        width: row.try_get("cover_width")?,
                        height: row.try_get("cover_height")?,
                        alt: row.try_get("cover_alt")?,
                    }),
                    None => None,
                },
                seo_title: row.try_get("seo_title")?,
                seo_description: row.try_get("seo_description")?,
                published_at,
                updated_at: desired_at.unwrap_or(published_at),
            })
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
        .map_err(db)?;
        let categories = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT DISTINCT c.slug, c.name, c.description FROM categories c \
             JOIN post_revision_categories rc ON rc.category_id=c.id \
             JOIN posts p ON p.desired_published_revision_id=rc.revision_id ORDER BY c.name, c.slug",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?
        .into_iter()
        .map(|(slug, name, description)| SnapshotCategory {
            slug,
            name,
            description,
        })
        .collect();
        let tags = sqlx::query_as::<_, (String, String)>(
            "SELECT DISTINCT t.slug, t.name FROM tags t \
             JOIN post_revision_tags rt ON rt.tag_id=t.id \
             JOIN posts p ON p.desired_published_revision_id=rt.revision_id ORDER BY t.name, t.slug",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?
        .into_iter()
        .map(|(slug, name)| SnapshotTag { slug, name })
        .collect();
        let profile = crate::profile::export_profile(&mut tx).await?;
        let media = sqlx::query_as::<_, (Uuid, String, i32, i32)>(
            "SELECT m.id, m.mime_type, m.width, m.height FROM media_assets m WHERE m.status='ready' AND m.id IN ( \
               SELECT r.cover_media_id FROM posts p JOIN post_revisions r ON r.id=p.desired_published_revision_id \
               UNION SELECT pm.media_id FROM posts p JOIN post_revision_media pm ON pm.revision_id=p.desired_published_revision_id \
               UNION SELECT $1::uuid) ORDER BY m.id",
        )
        .bind(profile.as_ref().and_then(|p| p.avatar.as_ref()).map(|image| image.media_id))
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?
        .into_iter()
        .map(|(id, mime_type, width, height)| SnapshotMedia {
            id,
            mime_type,
            width,
            height,
        })
        .collect();
        tx.commit().await.map_err(db)?;
        Ok(Some(ExportedSite {
            posts,
            categories,
            tags,
            profile,
            media,
        }))
    }

    #[tracing::instrument(name = "db.publish_activate", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn activate(
        &self,
        claimed: &ClaimedJob,
        manifest_key: &str,
        artifact_prefix: &str,
        posts: &[(Uuid, Uuid)],
    ) -> Result<Activation> {
        let _timer = METRICS.db_operation("publish_activate");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (desired, owned) = lock_job(&mut tx, claimed).await?;
        if !owned {
            return Ok(Activation::LeaseLost);
        }
        if desired != Some(claimed.generation) {
            finish(&mut tx, claimed.id, "superseded", None).await?;
            tx.commit().await.map_err(db)?;
            return Ok(Activation::Superseded);
        }
        let (post_ids, revision_ids): (Vec<Uuid>, Vec<Uuid>) = posts.iter().copied().unzip();
        sqlx::query(
            "INSERT INTO site_generation_posts (generation, post_id, revision_id) \
             SELECT $1, post_id, revision_id FROM unnest($2::uuid[], $3::uuid[]) AS t(post_id, revision_id)",
        )
        .bind(claimed.generation)
        .bind(&post_ids)
        .bind(&revision_ids)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        sqlx::query("UPDATE publish_jobs SET status='retired' WHERE status='active'")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query(
            "UPDATE publish_jobs SET status='active', manifest_key=$2, artifact_prefix=$3, lease_token=NULL, \
             lease_expires_at=NULL, finished_at=now(), error_message=NULL WHERE id=$1",
        )
        .bind(claimed.id)
        .bind(manifest_key)
        .bind(artifact_prefix)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        sqlx::query(
            "UPDATE site_state SET active_generation=$1, activated_at=now(),
               active_avatar_media_id=(SELECT p.avatar_media_id FROM profiles p ORDER BY p.updated_at DESC, p.id LIMIT 1)
             WHERE singleton",
        )
        .bind(claimed.generation)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Activation::Activated)
    }

    #[tracing::instrument(name = "db.publish_supersede", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn supersede(&self, claimed: &ClaimedJob) -> Result<()> {
        let _timer = METRICS.db_operation("publish_supersede");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (_, owned) = lock_job(&mut tx, claimed).await?;
        if owned {
            finish(&mut tx, claimed.id, "superseded", None).await?;
        }
        tx.commit().await.map_err(db)?;
        Ok(())
    }

    #[tracing::instrument(name = "db.publish_fail", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn fail(
        &self,
        claimed: &ClaimedJob,
        error: &str,
        max_attempts: i32,
        retry_delay: Duration,
    ) -> Result<()> {
        let _timer = METRICS.db_operation("publish_fail");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (desired, owned) = lock_job(&mut tx, claimed).await?;
        if !owned {
            return Ok(());
        }
        if desired != Some(claimed.generation) {
            finish(&mut tx, claimed.id, "superseded", Some(error)).await?;
        } else if claimed.attempt < max_attempts {
            // 仍为最新目标且未用尽重试：退避后重新排队，保留失败原因供后台查看。
            sqlx::query(
                "UPDATE publish_jobs SET status='queued', lease_token=NULL, lease_expires_at=NULL, error_message=$2, \
                 retry_after=now() + $3::interval WHERE id=$1",
            )
            .bind(claimed.id)
            .bind(error)
            .bind(lease_interval(retry_delay))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        } else {
            finish(&mut tx, claimed.id, "failed", Some(error)).await?;
        }
        tx.commit().await.map_err(db)?;
        Ok(())
    }

    #[tracing::instrument(name = "db.active_site", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn active_site(&self) -> Result<Option<ActiveSite>> {
        let _timer = METRICS.db_operation("active_site");
        let row: Option<(i64, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT s.active_generation, j.manifest_key, j.artifact_prefix FROM site_state s \
             JOIN publish_jobs j ON j.generation=s.active_generation WHERE s.singleton",
        )
        .fetch_optional(&mut *acquire(&self.pool).await.map_err(db)?)
        .await
        .map_err(db)?;
        Ok(match row {
            Some((generation, Some(manifest_key), Some(artifact_prefix))) => Some(ActiveSite {
                generation,
                manifest_key,
                artifact_prefix,
            }),
            _ => None,
        })
    }
}
