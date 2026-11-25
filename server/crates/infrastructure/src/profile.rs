//! 个人资料的管理接口和发布快照共用同一份有序数据。

use crate::{
    persistence::{audit_resource, begin, db},
    publishing::enqueue_site_publish,
};
use async_trait::async_trait;
use serde_json::json;
use silva_application::{
    ports::{ContentActor, ExportedProfile},
    profile::ProfileRepository,
};
use silva_domain::{
    profile::Profile,
    site::{SiteTarget, SnapshotImage},
};
use silva_shared::{
    error::{AppError, Conflict, FieldErrorCode, Result},
    ids,
    metrics::METRICS,
};
use sqlx::{PgConnection, PgPool, QueryBuilder, types::Json};
use uuid::Uuid;

pub struct PgProfileRepository {
    pool: PgPool,
}

impl PgProfileRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

struct StoredProfile {
    id: Uuid,
    profile: Profile,
    avatar: Option<SnapshotImage>,
}

/// 与已有导出约定一致，使用最近更新的一条资料。单条查询确保各子集合来自同一数据库快照。
async fn load(connection: &mut PgConnection) -> Result<Option<StoredProfile>> {
    let row = sqlx::query_as::<_, (Uuid, Json<Profile>, Option<Json<SnapshotImage>>)>(
        "SELECT p.id, jsonb_build_object(
           'display_name', p.display_name, 'headline', p.headline, 'bio_md', p.bio_md,
           'location', p.location_label, 'avatar_media_id', p.avatar_media_id,
           'experiences', ARRAY(SELECT jsonb_build_object('organization', e.organization, 'role', e.role,
               'description_md', e.description_md, 'started_on', e.started_on, 'ended_on', e.ended_on)
               FROM experiences e WHERE e.profile_id=p.id ORDER BY e.position, e.id),
           'skills', ARRAY(SELECT jsonb_build_object('name', s.name, 'category', s.category,
               'proficiency', s.proficiency_label) FROM skills s WHERE s.profile_id=p.id ORDER BY s.position, s.id),
           'links', ARRAY(SELECT jsonb_build_object('platform', l.platform, 'label', l.label, 'url', l.url)
               FROM social_links l WHERE l.profile_id=p.id ORDER BY l.position, l.id)),
           CASE WHEN m.id IS NOT NULL THEN jsonb_build_object('media_id', m.id, 'mime_type', m.mime_type,
               'width', m.width, 'height', m.height, 'alt', m.alt_text) END
         FROM profiles p LEFT JOIN media_assets m ON m.id=p.avatar_media_id AND m.status='ready'
         ORDER BY p.updated_at DESC, p.id LIMIT 1",
    )
    .fetch_optional(connection)
    .await
    .map_err(db)?;
    Ok(row.map(|(id, profile, avatar)| StoredProfile {
        id,
        profile: profile.0,
        avatar: avatar.map(|image| image.0),
    }))
}

pub(crate) async fn export_profile(
    connection: &mut PgConnection,
) -> Result<Option<ExportedProfile>> {
    Ok(load(connection).await?.map(|stored| ExportedProfile {
        profile: stored.profile,
        avatar: stored.avatar,
    }))
}

#[async_trait]
impl ProfileRepository for PgProfileRepository {
    async fn save(&self, profile: &Profile, actor: &ContentActor<'_>) -> Result<SiteTarget> {
        let _timer = METRICS.db_operation("profile_save");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        // 首次保存还没有资料行可锁；站点单例同时串行化首次创建、替换和发布登记。
        sqlx::query("SELECT singleton FROM site_state WHERE singleton FOR UPDATE")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let current = load(&mut tx).await?;
        let avatar_id = profile.avatar_media_id.map(|id| id.as_uuid());
        if let Some(id) = avatar_id {
            let status: String =
                sqlx::query_scalar("SELECT status FROM media_assets WHERE id=$1 FOR SHARE")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(db)?
                    .ok_or_else(|| {
                        AppError::invalid(
                            "avatar_media_id",
                            FieldErrorCode::InvalidValue,
                            "avatar media does not exist",
                        )
                    })?;
            if status != "ready" {
                return Err(AppError::Conflict(Conflict::MediaNotReady));
            }
        }
        if current
            .as_ref()
            .is_some_and(|stored| stored.profile == *profile)
        {
            // 后续文章等变更可能已经合并原任务；当前目标同样包含这份资料。
            let target: Option<(Uuid, i64)> = sqlx::query_as(
                "SELECT j.id, j.generation FROM site_state s JOIN publish_jobs j ON j.generation=s.desired_generation WHERE s.singleton",
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
            if let Some((publish_job_id, desired_generation)) = target {
                tx.commit().await.map_err(db)?;
                return Ok(SiteTarget {
                    publish_job_id,
                    desired_generation,
                });
            }
        }
        let id = current.map(|stored| stored.id).unwrap_or_else(ids::new_id);
        sqlx::query(
            "INSERT INTO profiles (id, display_name, headline, bio_md, location_label, avatar_media_id)
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (id) DO UPDATE SET display_name=EXCLUDED.display_name,
             headline=EXCLUDED.headline, bio_md=EXCLUDED.bio_md, location_label=EXCLUDED.location_label,
             avatar_media_id=EXCLUDED.avatar_media_id, updated_at=clock_timestamp()",
        )
        .bind(id)
        .bind(&profile.display_name)
        .bind(&profile.headline)
        .bind(&profile.bio_md)
        .bind(&profile.location)
        .bind(avatar_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        replace_entries(&mut tx, id, profile).await?;
        let target = enqueue_site_publish(&mut tx, "profile.update", Some(actor.user_id)).await?;
        audit_resource(
            &mut tx,
            Some(actor.user_id),
            "profile.update",
            "profile",
            id,
            actor.meta,
            json!({"generation": target.desired_generation, "publish_job_id": target.publish_job_id}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(target)
    }
}

async fn replace_entries(connection: &mut PgConnection, id: Uuid, profile: &Profile) -> Result<()> {
    for query in [
        "DELETE FROM experiences WHERE profile_id=$1",
        "DELETE FROM skills WHERE profile_id=$1",
        "DELETE FROM social_links WHERE profile_id=$1",
    ] {
        sqlx::query(query)
            .bind(id)
            .execute(&mut *connection)
            .await
            .map_err(db)?;
    }
    if !profile.experiences.is_empty() {
        let mut insert = QueryBuilder::new(
            "INSERT INTO experiences (id, profile_id, position, organization, role, description_md, started_on, ended_on) ",
        );
        insert.push_values(
            profile.experiences.iter().enumerate(),
            |mut row, (position, entry)| {
                row.push_bind(ids::new_id())
                    .push_bind(id)
                    .push_bind(position as i32)
                    .push_bind(&entry.organization)
                    .push_bind(&entry.role)
                    .push_bind(&entry.description_md)
                    .push_bind(entry.started_on)
                    .push_bind(entry.ended_on);
            },
        );
        insert.build().execute(&mut *connection).await.map_err(db)?;
    }
    if !profile.skills.is_empty() {
        let mut insert = QueryBuilder::new(
            "INSERT INTO skills (id, profile_id, position, name, category, proficiency_label) ",
        );
        insert.push_values(
            profile.skills.iter().enumerate(),
            |mut row, (position, entry)| {
                row.push_bind(ids::new_id())
                    .push_bind(id)
                    .push_bind(position as i32)
                    .push_bind(&entry.name)
                    .push_bind(&entry.category)
                    .push_bind(&entry.proficiency);
            },
        );
        insert.build().execute(&mut *connection).await.map_err(db)?;
    }
    if !profile.links.is_empty() {
        let mut insert = QueryBuilder::new(
            "INSERT INTO social_links (id, profile_id, position, platform, label, url) ",
        );
        insert.push_values(
            profile.links.iter().enumerate(),
            |mut row, (position, entry)| {
                row.push_bind(ids::new_id())
                    .push_bind(id)
                    .push_bind(position as i32)
                    .push_bind(&entry.platform)
                    .push_bind(&entry.label)
                    .push_bind(&entry.url);
            },
        );
        insert.build().execute(&mut *connection).await.map_err(db)?;
    }
    Ok(())
}
