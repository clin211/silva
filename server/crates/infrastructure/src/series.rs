//! 系列读取、编辑和发布。所有目录写入共用父行的编辑锁。
mod outline;
use crate::{
    persistence::{acquire, audit_resource, begin, db},
    public_site::advance_generation,
};
use async_trait::async_trait;
use serde_json::json;
use silva_application::series::{
    Actor, Detail, Edit, Edited, Filter, Item, Section, SeriesRepository, Sort, Summary,
};
use silva_domain::{
    series::Metadata,
    site::{PublicationResult, SiteVersion},
};
use silva_shared::{
    error::{AppError, Conflict, FieldErrorCode, Result},
    ids::{self, EntityId},
    pagination::{Page, PageRequest},
};
use sqlx::{PgConnection, PgPool, types::Json};
use uuid::Uuid;

pub(crate) struct PgSeriesRepository(pub(crate) PgPool);

const SUMMARY: &str = "SELECT s.*, (s.first_published_at IS NOT NULL) AS slug_locked, s.published_at IS NOT NULL AND counts.published_post_count > 0 AS is_live,
    counts.item_count, counts.published_post_count
    FROM series s
    CROSS JOIN LATERAL (SELECT count(*) AS item_count, count(pub.id) AS published_post_count
       FROM series_items i LEFT JOIN published_posts pub ON pub.post_id=i.post_id WHERE i.series_id=s.id) counts";

async fn detail(conn: &mut PgConnection, id: Uuid) -> Result<Detail> {
    // 汇总数、线上修订和条目来自同一语句的 MVCC 快照，文章可同时发布。
    let sql = format!(
        "SELECT to_jsonb(x),
        (SELECT coalesce(jsonb_agg(jsonb_build_object('id',sec.id,'title',sec.title,
            'description',sec.description,'position',sec.position,'items','[]'::jsonb)
            ORDER BY sec.position,sec.id), '[]') FROM series_sections sec WHERE sec.series_id=x.id),
        (SELECT coalesce(jsonb_agg(to_jsonb(i) || jsonb_build_object('title',r.title,
            'slug',p.slug,'current_revision_id',r.id,
            'published_revision_id',pub.id)
            ORDER BY i.position,i.id), '[]')
         FROM series_items i JOIN posts p ON p.id=i.post_id
         JOIN post_revisions r ON r.id=p.current_revision_id
         LEFT JOIN published_posts pub ON pub.post_id=p.id
         WHERE i.series_id=x.id)
        FROM ({SUMMARY} WHERE s.id=$1) x"
    );
    let (Json(summary), Json(mut sections), Json(rows)) =
        sqlx::query_as::<_, (Json<Summary>, Json<Vec<Section>>, Json<Vec<Item>>)>(&sql)
            .bind(id)
            .fetch_optional(conn)
            .await
            .map_err(db)?
            .ok_or(AppError::NotFound)?;
    let mut items = Vec::new();
    for item in rows {
        if let Some(section) = item.section_id {
            let group = sections
                .iter_mut()
                .find(|s| s.id == section)
                .ok_or_else(|| AppError::Internal(anyhow::anyhow!("missing series section")))?;
            group.items.push(item);
        } else {
            items.push(item);
        }
    }
    Ok(Detail {
        summary,
        sections,
        items,
    })
}

async fn lock(conn: &mut PgConnection, id: Uuid) -> Result<()> {
    sqlx::query("SELECT id FROM series WHERE id=$1 FOR NO KEY UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
        .map_err(db)?
        .ok_or(AppError::NotFound)?;
    Ok(())
}

fn check_edit(current: i64, expected: i64) -> Result<()> {
    if current != expected {
        return Err(AppError::Conflict(Conflict::SeriesEditConflict));
    }
    Ok(())
}
fn check_publication(current: i64, expected: i64) -> Result<()> {
    if current != expected {
        return Err(AppError::Conflict(Conflict::PublicationConflict));
    }
    Ok(())
}

async fn audit(
    conn: &mut PgConnection,
    id: Uuid,
    action: &str,
    actor: &Actor<'_>,
    target: Option<SiteVersion>,
) -> Result<()> {
    audit_resource(
        conn,
        Some(actor.principal.user.id),
        action,
        "series",
        id,
        actor.meta,
        json!({"generation":target.map(|t| t.generation)}),
    )
    .await
}

async fn references(conn: &mut PgConnection, input: &Metadata, actor: &Actor<'_>) -> Result<()> {
    if let Some(category) = input.category_id {
        sqlx::query("SELECT id FROM categories WHERE id=$1 FOR KEY SHARE")
            .bind(category.as_uuid())
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?
            .ok_or(AppError::NotFound)?;
    }
    if let Some(cover) = input.cover_media_id {
        let (status, owner): (String, Option<Uuid>) =
            sqlx::query_as("SELECT status,uploaded_by FROM media_assets WHERE id=$1 FOR SHARE")
                .bind(cover.as_uuid())
                .fetch_optional(conn)
                .await
                .map_err(db)?
                .ok_or(AppError::NotFound)?;
        if actor.principal.require("media.manage").is_err() {
            actor.principal.require("media.manage_own")?;
            if owner != Some(actor.principal.user.id) {
                return Err(AppError::NotFound);
            }
        }
        if status != "ready" {
            return Err(AppError::Conflict(Conflict::MediaNotReady));
        }
    }
    Ok(())
}

fn write_error(error: sqlx::Error) -> AppError {
    match error.as_database_error().and_then(|e| e.constraint()) {
        Some("series_slug_key") => AppError::Conflict(Conflict::SlugInUse),
        Some("series_items_post_id_key") => AppError::Conflict(Conflict::PostAlreadyInSeries),
        Some("series_items_post_id_fkey") => AppError::NotFound,
        _ => db(error),
    }
}

async fn update_metadata(
    conn: &mut PgConnection,
    current: &Summary,
    patch: silva_domain::series::MetadataPatch,
    actor: &Actor<'_>,
) -> Result<bool> {
    let mut input = current.metadata.clone();
    patch.apply(&mut input);
    input.normalize()?;
    if input == current.metadata {
        return Ok(false);
    }
    if input.slug != current.metadata.slug {
        if current.slug_locked {
            return Err(AppError::Conflict(Conflict::SlugLocked));
        }
        sqlx::query("UPDATE series SET slug=$2 WHERE id=$1")
            .bind(current.id)
            .bind(&input.slug)
            .execute(&mut *conn)
            .await
            .map_err(write_error)?;
    }
    references(conn, &input, actor).await?;
    sqlx::query("UPDATE series SET title=$2,description=$3,cover_media_id=$4,category_id=$5,completion_status=$6,
        is_featured=$7,sort_order=$8,seo_title=$9,seo_description=$10 WHERE id=$1")
        .bind(current.id)
        .bind(input.title)
        .bind(input.description)
        .bind(input.cover_media_id.map(EntityId::as_uuid))
        .bind(input.category_id.map(EntityId::as_uuid))
        .bind(input.completion_status.as_str())
        .bind(input.is_featured)
        .bind(input.sort_order)
        .bind(input.seo_title)
        .bind(input.seo_description)
        .execute(conn)
        .await
        .map_err(db)?;
    Ok(true)
}

#[async_trait]
impl SeriesRepository for PgSeriesRepository {
    async fn list(&self, page: &PageRequest, filter: &Filter) -> Result<Page<Summary>> {
        const FILTER: &str = "WHERE ($1::text IS NULL OR strpos(lower(s.slug),lower($1))>0 OR strpos(lower(s.title),lower($1))>0)
            AND ($2::uuid IS NULL OR s.category_id=$2) AND ($3::bool IS NULL OR (s.published_at IS NOT NULL)=$3)
            AND ($4::text IS NULL OR s.completion_status=$4)";
        let order = match filter.sort {
            Sort::SortOrderAsc => "s.sort_order",
            Sort::UpdatedDesc => "s.updated_at DESC",
            Sort::CreatedDesc => "s.created_at DESC",
        };
        let mut tx = begin(&self.0).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let completion = filter.completion_status.map(|s| s.as_str());
        let total = sqlx::query_scalar(&format!("SELECT count(*) FROM series s {FILTER}"))
            .bind(&filter.q)
            .bind(filter.category_id)
            .bind(filter.published)
            .bind(completion)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let items = sqlx::query_scalar::<_, Json<Summary>>(&format!("SELECT to_jsonb(s) FROM ({SUMMARY} {FILTER}) s ORDER BY {order},s.id LIMIT $5 OFFSET $6"))
            .bind(&filter.q)
            .bind(filter.category_id)
            .bind(filter.published)
            .bind(completion)
            .bind(i64::from(page.per_page))
            .bind(page.offset())
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?
            .into_iter()
            .map(|s| s.0)
            .collect();
        tx.commit().await.map_err(db)?;
        Ok(Page {
            items,
            pagination: page.metadata(total),
        })
    }
    async fn detail(&self, id: Uuid) -> Result<Detail> {
        detail(&mut *acquire(&self.0).await.map_err(db)?, id).await
    }
    async fn create(&self, input: Metadata, actor: Actor<'_>) -> Result<Detail> {
        let mut tx = begin(&self.0).await.map_err(db)?;
        references(&mut tx, &input, &actor).await?;
        let id = ids::new_id();
        sqlx::query("INSERT INTO series(id,slug,title,description,cover_media_id,category_id,completion_status,is_featured,sort_order,seo_title,seo_description,created_by)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(id)
            .bind(input.slug)
            .bind(input.title)
            .bind(input.description)
            .bind(input.cover_media_id.map(EntityId::as_uuid))
            .bind(input.category_id.map(EntityId::as_uuid))
            .bind(input.completion_status.as_str())
            .bind(input.is_featured)
            .bind(input.sort_order)
            .bind(input.seo_title)
            .bind(input.seo_description)
            .bind(actor.principal.user.id)
            .execute(&mut *tx)
            .await
            .map_err(write_error)?;
        audit(&mut tx, id, "series.create", &actor, None).await?;
        let result = detail(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }
    async fn edit(&self, id: Uuid, edit: Edit, actor: Actor<'_>) -> Result<Edited> {
        let mut tx = begin(&self.0).await.map_err(db)?;
        lock(&mut tx, id).await?;
        let current = detail(&mut tx, id).await?;
        check_edit(current.summary.edit_version, edit.version())?;
        let (changed, action) = match edit {
            Edit::Metadata(patch) => (
                update_metadata(&mut tx, &current.summary, patch, &actor).await?,
                "series.update",
            ),
            Edit::Outline(input) => (
                outline::replace(&mut tx, &current, input).await?,
                "series.outline",
            ),
            Edit::Append { section, item, .. } => {
                outline::append(&mut tx, &current, section, item).await?;
                (true, "series.item.add")
            }
            Edit::Remove { post, .. } => {
                outline::remove(&mut tx, id, post).await?;
                (true, "series.item.remove")
            }
        };
        let mut publication = None;
        let series = if changed {
            sqlx::query("UPDATE series SET edit_version=edit_version+1 WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            if current.summary.published_at.is_some() {
                publication = Some(advance_generation(&mut tx).await?);
            }
            audit(&mut tx, id, action, &actor, publication).await?;
            detail(&mut tx, id).await?
        } else {
            current
        };
        tx.commit().await.map_err(write_error)?;
        Ok(Edited {
            series,
            publication,
        })
    }
    async fn publish(
        &self,
        id: Uuid,
        version: i64,
        published: bool,
        actor: Actor<'_>,
    ) -> Result<PublicationResult> {
        let mut tx = begin(&self.0).await.map_err(db)?;
        let (stored, was_published): (i64, bool) = sqlx::query_as(
            "SELECT publication_version,published_at IS NOT NULL FROM series WHERE id=$1 FOR NO KEY UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .ok_or(AppError::NotFound)?;
        check_publication(stored, version)?;
        if was_published == published {
            return Err(AppError::Conflict(if published {
                Conflict::AlreadyPublished
            } else {
                Conflict::AlreadyUnpublished
            }));
        }
        sqlx::query("UPDATE series SET publication_version=publication_version+1,published_at=CASE WHEN $2 THEN now() END,
            first_published_at=COALESCE(first_published_at,CASE WHEN $2 THEN now() END) WHERE id=$1")
            .bind(id)
            .bind(published)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let action = if published {
            "series.publish"
        } else {
            "series.unpublish"
        };
        let target = advance_generation(&mut tx).await?;
        audit(&mut tx, id, action, &actor, Some(target)).await?;
        tx.commit().await.map_err(db)?;
        Ok(PublicationResult {
            generation: target.generation,
            publication_version: version + 1,
        })
    }
    async fn delete(
        &self,
        id: Uuid,
        version: i64,
        publication_version: i64,
        actor: Actor<'_>,
    ) -> Result<()> {
        let mut tx = begin(&self.0).await.map_err(db)?;
        lock(&mut tx, id).await?;
        let (edit, pub_version, in_use): (i64, i64, bool) = sqlx::query_as(
            "SELECT edit_version,publication_version,published_at IS NOT NULL FROM series WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        check_edit(edit, version)?;
        check_publication(pub_version, publication_version)?;
        if in_use {
            return Err(AppError::Conflict(Conflict::SeriesInUse));
        }
        sqlx::query("DELETE FROM series WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit(&mut tx, id, "series.delete", &actor, None).await?;
        tx.commit().await.map_err(db)
    }
}
