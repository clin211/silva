//! 分类与标签的 PostgreSQL 仓储。两者结构相同，按种类选择表名与关联表。

use crate::{
    persistence::{acquire, audit_resource, begin, db},
    publishing::enqueue_site_publish,
};
use async_trait::async_trait;
use serde_json::json;
use silva_application::ports::{ContentActor, TaxonomyItem, TaxonomyKind, TaxonomyRepository};
use silva_domain::{
    content::{Category, Tag, TaxonomyInput},
    site::SiteTarget,
};
use silva_shared::{
    error::{AppError, Conflict, Result},
    metrics::METRICS,
};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow};
use uuid::Uuid;

#[derive(Clone)]
pub struct PgTaxonomyRepository {
    pool: PgPool,
}

impl PgTaxonomyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 表名、修订关联表与外键列。均为常量，可安全拼入 SQL。
fn tables(kind: TaxonomyKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        TaxonomyKind::Category => ("categories", "post_revision_categories", "category_id"),
        TaxonomyKind::Tag => ("tags", "post_revision_tags", "tag_id"),
    }
}

fn select(kind: TaxonomyKind) -> String {
    let (table, link, column) = tables(kind);
    let description = match kind {
        TaxonomyKind::Category => "t.description",
        TaxonomyKind::Tag => "NULL::text AS description",
    };
    format!(
        "SELECT t.id, t.slug, t.name, {description}, t.created_at, t.updated_at, \
         (SELECT count(*) FROM {link} x WHERE x.{column}=t.id) AS revision_count FROM {table} t"
    )
}

fn read(kind: TaxonomyKind, row: &PgRow) -> std::result::Result<TaxonomyItem, sqlx::Error> {
    Ok(match kind {
        TaxonomyKind::Category => TaxonomyItem::Category(Category {
            id: row.try_get("id")?,
            slug: row.try_get("slug")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            revision_count: row.try_get("revision_count")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        }),
        TaxonomyKind::Tag => TaxonomyItem::Tag(Tag {
            id: row.try_get("id")?,
            slug: row.try_get("slug")?,
            name: row.try_get("name")?,
            revision_count: row.try_get("revision_count")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        }),
    })
}

async fn find(
    connection: &mut PgConnection,
    kind: TaxonomyKind,
    id: Uuid,
) -> Result<Option<TaxonomyItem>> {
    sqlx::query(&format!("{} WHERE t.id=$1", select(kind)))
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(db)?
        .as_ref()
        .map(|row| read(kind, row))
        .transpose()
        .map_err(db)
}

async fn audit(
    connection: &mut PgConnection,
    kind: TaxonomyKind,
    actor: &ContentActor<'_>,
    action: &str,
    id: Uuid,
    metadata: serde_json::Value,
) -> Result<()> {
    let resource = kind.resource_type();
    audit_resource(
        connection,
        Some(actor.user_id),
        &format!("{resource}.{action}"),
        resource,
        id,
        actor.meta,
        metadata,
    )
    .await
}

#[async_trait]
impl TaxonomyRepository for PgTaxonomyRepository {
    #[tracing::instrument(name = "db.taxonomy_list", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn list(&self, kind: TaxonomyKind) -> Result<Vec<TaxonomyItem>> {
        let _timer = METRICS.db_operation("taxonomy_list");
        sqlx::query(&format!("{} ORDER BY t.name, t.id", select(kind)))
            .fetch_all(&mut *acquire(&self.pool).await.map_err(db)?)
            .await
            .map_err(db)?
            .iter()
            .map(|row| read(kind, row))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }

    #[tracing::instrument(name = "db.taxonomy_find", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn find(&self, kind: TaxonomyKind, id: Uuid) -> Result<Option<TaxonomyItem>> {
        let _timer = METRICS.db_operation("taxonomy_find");
        find(&mut *acquire(&self.pool).await.map_err(db)?, kind, id).await
    }

    #[tracing::instrument(name = "db.taxonomy_create", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn create(
        &self,
        kind: TaxonomyKind,
        id: Uuid,
        slug: &str,
        input: &TaxonomyInput,
        actor: &ContentActor<'_>,
    ) -> Result<TaxonomyItem> {
        let _timer = METRICS.db_operation("taxonomy_create");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let query = match kind {
            TaxonomyKind::Category => sqlx::query(
                "INSERT INTO categories (id, slug, name, description) VALUES ($1,$2,$3,$4)",
            )
            .bind(id)
            .bind(slug)
            .bind(&input.name)
            .bind(&input.description),
            TaxonomyKind::Tag => sqlx::query("INSERT INTO tags (id, slug, name) VALUES ($1,$2,$3)")
                .bind(id)
                .bind(slug)
                .bind(&input.name),
        };
        query.execute(&mut *tx).await.map_err(db)?;
        audit(&mut tx, kind, actor, "create", id, json!({"slug": slug})).await?;
        let created = find(&mut tx, kind, id).await?.ok_or(AppError::NotFound)?;
        tx.commit().await.map_err(db)?;
        Ok(created)
    }

    #[tracing::instrument(name = "db.taxonomy_update", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn update(
        &self,
        kind: TaxonomyKind,
        id: Uuid,
        input: &TaxonomyInput,
        actor: &ContentActor<'_>,
    ) -> Result<(TaxonomyItem, Option<SiteTarget>)> {
        let _timer = METRICS.db_operation("taxonomy_update");
        let (_, link, column) = tables(kind);
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let current: (String, Option<String>) = match kind {
            TaxonomyKind::Category => {
                sqlx::query_as("SELECT name, description FROM categories WHERE id=$1 FOR UPDATE")
            }
            TaxonomyKind::Tag => {
                sqlx::query_as("SELECT name, NULL::text FROM tags WHERE id=$1 FOR UPDATE")
            }
        }
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(AppError::NotFound)?;
        let changed = current.0 != input.name || current.1 != input.description;
        let mut target = None;
        if changed {
            match kind {
                TaxonomyKind::Category => {
                    sqlx::query("UPDATE categories SET name=$2, description=$3 WHERE id=$1")
                        .bind(id)
                        .bind(&input.name)
                        .bind(&input.description)
                }
                TaxonomyKind::Tag => sqlx::query("UPDATE tags SET name=$2 WHERE id=$1")
                    .bind(id)
                    .bind(&input.name),
            }
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            // 名称出现在期望公开的文章中时，公开站点需要重新构建。
            let public: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS (SELECT 1 FROM posts p JOIN {link} x ON x.revision_id=p.desired_published_revision_id WHERE x.{column}=$1)"
            ))
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            if public {
                target = Some(
                    enqueue_site_publish(
                        &mut tx,
                        &format!("{}.update", kind.resource_type()),
                        Some(actor.user_id),
                    )
                    .await?,
                );
            }
            audit(
                &mut tx,
                kind,
                actor,
                "update",
                id,
                json!({"generation": target.map(|t| t.desired_generation)}),
            )
            .await?;
        }
        let updated = find(&mut tx, kind, id).await?.ok_or(AppError::NotFound)?;
        tx.commit().await.map_err(db)?;
        Ok((updated, target))
    }

    #[tracing::instrument(name = "db.taxonomy_delete", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn delete(&self, kind: TaxonomyKind, id: Uuid, actor: &ContentActor<'_>) -> Result<()> {
        let _timer = METRICS.db_operation("taxonomy_delete");
        let (table, link, column) = tables(kind);
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query(&format!("SELECT id FROM {table} WHERE id=$1 FOR UPDATE"))
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .ok_or(AppError::NotFound)?;
        let used: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS (SELECT 1 FROM {link} WHERE {column}=$1)"
        ))
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if used {
            return Err(AppError::Conflict(Conflict::TaxonomyInUse));
        }
        sqlx::query(&format!("DELETE FROM {table} WHERE id=$1"))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|error| match &error {
                // 检查后才出现的引用由外键拒绝。
                sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
                    AppError::Conflict(Conflict::TaxonomyInUse)
                }
                _ => db(error),
            })?;
        audit(&mut tx, kind, actor, "delete", id, json!({})).await?;
        tx.commit().await.map_err(db)?;
        Ok(())
    }
}
