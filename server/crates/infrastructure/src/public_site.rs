//! 发布事务与公开读取共享同一份 PostgreSQL 发布数据。

use crate::persistence::{begin, db};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use silva_application::ports::{ExportedSite, PublicSiteRepository};
use silva_domain::{
    content::Heading,
    site::{
        SiteVersion, SnapshotCategory, SnapshotImage, SnapshotMedia, SnapshotPost, SnapshotTag,
    },
};
use silva_shared::error::Result;
use sqlx::{PgConnection, PgPool, Row, types::Json};
use uuid::Uuid;

#[derive(Clone)]
pub struct PgPublicSiteRepository {
    pool: PgPool,
}

impl PgPublicSiteRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 在公开数据的写事务内递增版本，行锁使版本按事务提交顺序生效。
pub(crate) async fn advance_generation(connection: &mut PgConnection) -> Result<SiteVersion> {
    let generation = sqlx::query_scalar(
        "UPDATE site_state SET generation=generation+1, updated_at=clock_timestamp() WHERE singleton RETURNING generation",
    )
    .fetch_one(connection)
    .await
    .map_err(db)?;
    Ok(SiteVersion { generation })
}

#[async_trait]
impl PublicSiteRepository for PgPublicSiteRepository {
    async fn media_storage_key(&self, id: Uuid) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT storage_key FROM media_assets WHERE id=$1 AND status='ready'")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db)
    }
    async fn generation(&self) -> Result<i64> {
        sqlx::query_scalar("SELECT generation FROM site_state WHERE singleton")
            .fetch_one(&self.pool)
            .await
            .map_err(db)
    }

    /// 版本号与所有公开数据在同一事务快照中读取。
    #[tracing::instrument(name = "db.public_snapshot", skip_all)]
    async fn export(&self) -> Result<(i64, DateTime<Utc>, ExportedSite)> {
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let (generation, updated): (i64, DateTime<Utc>) =
            sqlx::query_as("SELECT generation,updated_at FROM site_state WHERE singleton")
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        let posts = sqlx::query(
            "SELECT p.id, r.id AS revision_id, p.slug, r.title, r.excerpt, r.rendered_html, r.rendered_headings, \
               r.is_featured, r.seo_title, r.seo_description, p.first_published_at, r.published_at, \
               m.id AS cover_id, m.mime_type AS cover_mime, m.width AS cover_width, m.height AS cover_height, m.alt_text AS cover_alt, \
               ARRAY(SELECT c.slug FROM published_post_categories rc JOIN categories c ON c.id=rc.category_id \
                     WHERE rc.revision_id=r.id ORDER BY c.name, c.slug) AS categories, \
               ARRAY(SELECT t.slug FROM published_post_tags rt JOIN tags t ON t.id=rt.tag_id \
                     WHERE rt.revision_id=r.id ORDER BY t.name, t.slug) AS tags \
             FROM posts p JOIN published_posts r ON r.post_id=p.id \
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
            let revision_published_at: DateTime<Utc> = row.try_get("published_at")?;
            let cover_id: Option<Uuid> = row.try_get("cover_id")?;
            let published_at = first_published.unwrap_or(revision_published_at);
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
                updated_at: revision_published_at,
            })
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
        .map_err(db)?;
        let categories = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT DISTINCT c.slug, c.name, c.description FROM categories c \
             JOIN published_post_categories rc ON rc.category_id=c.id \
             ORDER BY c.name, c.slug",
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
             JOIN published_post_tags rt ON rt.tag_id=t.id \
             ORDER BY t.name, t.slug",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?
        .into_iter()
        .map(|(slug, name)| SnapshotTag { slug, name })
        .collect();
        let series: Vec<_> = sqlx::query_scalar::<_, Json<silva_domain::site::SnapshotSeries>>(
            include_str!("series/export.sql"),
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?
        .into_iter()
        .map(|Json(s)| s)
        .collect();
        let covers: Vec<_> = series
            .iter()
            .filter_map(|s| s.cover.as_ref().map(|m| m.media_id))
            .collect();
        let profile = crate::profile::export_profile(&mut tx).await?;
        let media = sqlx::query_as::<_, (Uuid, String, i32, i32)>(
            "SELECT m.id, m.mime_type, m.width, m.height FROM media_assets m WHERE m.status='ready' AND m.id IN ( \
               SELECT r.cover_media_id FROM published_posts r \
               UNION SELECT pm.media_id FROM published_post_media pm \
               UNION SELECT $1::uuid UNION SELECT unnest($2::uuid[])) ORDER BY m.id",
        )
        .bind(profile.as_ref().and_then(|p| p.avatar.as_ref()).map(|image| image.media_id))
        .bind(covers)
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
        Ok((
            generation,
            updated,
            ExportedSite {
                series,
                posts,
                categories,
                tags,
                profile,
                media,
            },
        ))
    }
}
