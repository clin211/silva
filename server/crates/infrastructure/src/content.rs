//! 文章编辑与公开修订的 PostgreSQL 仓储。

use crate::{
    persistence::{acquire, audit_resource, begin, db},
    public_site::advance_generation,
};
use async_trait::async_trait;
use serde_json::json;
use silva_application::ports::{
    ContentActor, ContentRepository, MediaReference, NewPost, NewRevision, PostAccess,
    SavedRevision,
};
use silva_domain::{
    content::{
        Heading, Post, PostCategoryCount, PostCategoryCounts, PostFilter, PostSort, PostSummary,
        Publication, Revision, RevisionContent, RevisionSummary, TaxonomyRef, UserRef,
    },
    media::MediaStatus,
    site::PublicationResult,
};
use silva_shared::{
    error::{AppError, Conflict, FieldErrorCode, Result},
    metrics::METRICS,
    pagination::{Page, PageRequest},
};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow, types::Json};
use uuid::Uuid;

#[derive(Clone)]
pub struct PgContentRepository {
    pool: PgPool,
}

impl PgContentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(serde::Deserialize)]
struct TaxonomyRow {
    id: Uuid,
    slug: String,
    name: String,
}

fn user(id: Option<Uuid>, name: Option<String>) -> Option<UserRef> {
    Some(UserRef {
        id: id?,
        display_name: name.unwrap_or_default(),
    })
}

const REVISION_SELECT: &str = "SELECT r.id, r.post_id, r.revision_no, r.title, r.excerpt, r.markdown_body, r.rendered_html, \
    r.rendered_headings, r.cover_media_id, r.is_featured, r.seo_title, r.seo_description, r.created_by, \
    cu.display_name AS created_by_name, r.created_at, \
    COALESCE((SELECT json_agg(json_build_object('id', c.id, 'slug', c.slug, 'name', c.name) ORDER BY c.name, c.slug) \
      FROM post_revision_categories rc JOIN categories c ON c.id=rc.category_id WHERE rc.revision_id=r.id), '[]') AS categories, \
    COALESCE((SELECT json_agg(json_build_object('id', t.id, 'slug', t.slug, 'name', t.name) ORDER BY t.name, t.slug) \
      FROM post_revision_tags rt JOIN tags t ON t.id=rt.tag_id WHERE rt.revision_id=r.id), '[]') AS tags, \
    ARRAY(SELECT pm.media_id FROM post_revision_media pm WHERE pm.revision_id=r.id ORDER BY pm.media_id) AS media_ids \
    FROM post_revisions r LEFT JOIN users cu ON cu.id=r.created_by";

fn read_revision(row: &PgRow) -> std::result::Result<Revision, sqlx::Error> {
    let refs = |rows: Vec<TaxonomyRow>| {
        rows.into_iter()
            .map(|row| TaxonomyRef {
                id: row.id,
                slug: row.slug,
                name: row.name,
            })
            .collect()
    };
    let Json(categories): Json<Vec<TaxonomyRow>> = row.try_get("categories")?;
    let Json(tags): Json<Vec<TaxonomyRow>> = row.try_get("tags")?;
    let Json(headings): Json<Vec<Heading>> = row.try_get("rendered_headings")?;
    let html: Option<String> = row.try_get("rendered_html")?;
    Ok(Revision {
        id: row.try_get("id")?,
        post_id: row.try_get("post_id")?,
        revision_no: row.try_get("revision_no")?,
        title: row.try_get("title")?,
        excerpt: row.try_get("excerpt")?,
        markdown_body: row.try_get("markdown_body")?,
        rendered_html: html.unwrap_or_default(),
        headings,
        categories: refs(categories),
        tags: refs(tags),
        cover_media_id: row.try_get("cover_media_id")?,
        media_ids: row.try_get("media_ids")?,
        is_featured: row.try_get("is_featured")?,
        seo_title: row.try_get("seo_title")?,
        seo_description: row.try_get("seo_description")?,
        created_by: user(row.try_get("created_by")?, row.try_get("created_by_name")?),
        created_at: row.try_get("created_at")?,
    })
}

async fn revision(
    connection: &mut PgConnection,
    post_id: Uuid,
    id: Uuid,
) -> Result<Option<Revision>> {
    sqlx::query(&format!("{REVISION_SELECT} WHERE r.post_id=$1 AND r.id=$2"))
        .bind(post_id)
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(db)?
        .as_ref()
        .map(read_revision)
        .transpose()
        .map_err(db)
}

const MEMBERSHIP: &str = "(SELECT jsonb_build_object('id',ss.id,'slug',ss.slug,'title',ss.title,
    'section_title',sec.title,'position',i.position) FROM series_items i JOIN series ss ON ss.id=i.series_id
    LEFT JOIN series_sections sec ON sec.id=i.section_id WHERE i.post_id=p.id)";

const POST_SELECT: &str = "p.id, p.slug, (p.first_published_at IS NOT NULL) AS slug_locked, p.author_id, au.display_name AS author_name, \
    p.current_revision_id, pub.id AS published_revision_id, pub.published_at, p.publication_version, \
    p.first_published_at, p.created_at, p.updated_at \
    FROM posts p LEFT JOIN users au ON au.id=p.author_id LEFT JOIN published_posts pub ON pub.post_id=p.id";

async fn post(connection: &mut PgConnection, id: Uuid) -> Result<Option<Post>> {
    let Some(row) = sqlx::query(&format!(
        "SELECT {MEMBERSHIP} AS series,{POST_SELECT} WHERE p.id=$1"
    ))
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?
    else {
        return Ok(None);
    };
    let current_revision_id: Uuid = row.try_get("current_revision_id").map_err(db)?;
    let current = revision(connection, id, current_revision_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("current revision is missing")))?;
    let read = || -> std::result::Result<Post, sqlx::Error> {
        Ok(Post {
            series: row.try_get::<Option<Json<_>>, _>("series")?.map(|j| j.0),
            id,
            slug: row.try_get("slug")?,
            slug_locked: row.try_get("slug_locked")?,
            author: user(row.try_get("author_id")?, row.try_get("author_name")?),
            current_revision_id,
            current_revision: current,
            publication: Publication {
                published_revision_id: row.try_get("published_revision_id")?,
                published_at: row.try_get("published_at")?,
                publication_version: row.try_get("publication_version")?,
                first_published_at: row.try_get("first_published_at")?,
            },
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    };
    read().map(Some).map_err(db)
}

/// 校验之后、写入之前被并发删除的引用由外键拒绝；按约束映射为对应字段的校验错误，
/// 与保存前校验的结果一致，而不是内部错误。
fn reference_error(error: sqlx::Error) -> AppError {
    let field = match &error {
        sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
            match database.constraint() {
                Some("post_revision_categories_category_id_fkey") => Some("category_ids"),
                Some("post_revision_tags_tag_id_fkey") => Some("tag_ids"),
                Some("post_revision_media_media_id_fkey") => Some("markdown_body"),
                Some("post_revisions_cover_media_fk") => Some("cover_media_id"),
                _ => None,
            }
        }
        _ => None,
    };
    match field {
        Some(field) => AppError::invalid(
            field,
            FieldErrorCode::InvalidValue,
            "the referenced item no longer exists",
        ),
        None => db(error),
    }
}

/// 插入修订及其分类、标签与正文媒体关联。
async fn insert_revision(
    connection: &mut PgConnection,
    post_id: Uuid,
    revision_no: i32,
    revision: &NewRevision,
) -> Result<()> {
    let content = &revision.content;
    sqlx::query(
        "INSERT INTO post_revisions (id, post_id, revision_no, title, excerpt, markdown_body, rendered_html, \
         rendered_headings, cover_media_id, is_featured, seo_title, seo_description, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(revision.id)
    .bind(post_id)
    .bind(revision_no)
    .bind(&content.title)
    .bind(&content.excerpt)
    .bind(&content.markdown_body)
    .bind(&revision.rendered.html)
    .bind(Json(&revision.rendered.headings))
    .bind(content.cover_media_id)
    .bind(content.is_featured)
    .bind(&content.seo_title)
    .bind(&content.seo_description)
    .bind(revision.created_by)
    .execute(&mut *connection)
    .await
    .map_err(reference_error)?;
    for (table, column, ids) in [
        (
            "post_revision_categories",
            "category_id",
            &content.category_ids,
        ),
        ("post_revision_tags", "tag_id", &content.tag_ids),
        ("post_revision_media", "media_id", &revision.body_media_ids),
    ] {
        sqlx::query(&format!(
            "INSERT INTO {table} (revision_id, {column}) SELECT $1, unnest($2::uuid[])"
        ))
        .bind(revision.id)
        .bind(ids)
        .execute(&mut *connection)
        .await
        .map_err(reference_error)?;
    }
    Ok(())
}

/// 读取修订的可比较内容，用于判断自动保存是否有变化。
async fn revision_content(connection: &mut PgConnection, id: Uuid) -> Result<RevisionContent> {
    let row = sqlx::query(
        "SELECT title, excerpt, markdown_body, cover_media_id, is_featured, seo_title, seo_description, \
         ARRAY(SELECT category_id FROM post_revision_categories WHERE revision_id=r.id) AS category_ids, \
         ARRAY(SELECT tag_id FROM post_revision_tags WHERE revision_id=r.id) AS tag_ids \
         FROM post_revisions r WHERE id=$1",
    )
    .bind(id)
    .fetch_one(connection)
    .await
    .map_err(db)?;
    let read = || -> std::result::Result<RevisionContent, sqlx::Error> {
        Ok(RevisionContent {
            title: row.try_get("title")?,
            excerpt: row.try_get("excerpt")?,
            markdown_body: row.try_get("markdown_body")?,
            category_ids: row.try_get("category_ids")?,
            tag_ids: row.try_get("tag_ids")?,
            cover_media_id: row.try_get("cover_media_id")?,
            is_featured: row.try_get("is_featured")?,
            seo_title: row.try_get("seo_title")?,
            seo_description: row.try_get("seo_description")?,
        })
    };
    read().map_err(db)
}

async fn audit(
    connection: &mut PgConnection,
    actor: &ContentActor<'_>,
    action: &str,
    resource_type: &str,
    id: Uuid,
    metadata: serde_json::Value,
) -> Result<()> {
    audit_resource(
        connection,
        Some(actor.user_id),
        action,
        resource_type,
        id,
        actor.meta,
        metadata,
    )
    .await
}

/// 文章列表的筛选条件；参数依次为关键字模式、作者、分类、标签、是否有公开修订、是否未分类。
const POST_FILTER: &str = "WHERE ($1::text IS NULL OR r.title ILIKE $1 ESCAPE '\\' OR p.slug ILIKE $1 ESCAPE '\\') \
    AND ($2::uuid IS NULL OR p.author_id=$2) \
    AND ($3::uuid IS NULL OR EXISTS (SELECT 1 FROM post_revision_categories x WHERE x.revision_id=r.id AND x.category_id=$3)) \
    AND ($4::uuid IS NULL OR EXISTS (SELECT 1 FROM post_revision_tags x WHERE x.revision_id=r.id AND x.tag_id=$4)) \
    AND ($5::boolean IS NULL OR (pub.id IS NOT NULL)=$5) \
    AND ($6::boolean IS NULL OR (NOT EXISTS (SELECT 1 FROM post_revision_categories x WHERE x.revision_id=r.id)) = $6) AND ($7::uuid IS NULL OR si.series_id=$7) AND ($8::bool IS NULL OR (si.series_id IS NOT NULL)=$8)";

const POST_LIST_FROM: &str = "FROM posts p LEFT JOIN series_items si ON si.post_id=p.id JOIN post_revisions r ON r.id=p.current_revision_id \
    LEFT JOIN users au ON au.id=p.author_id LEFT JOIN published_posts pub ON pub.post_id=p.id";

fn read_summary(row: &PgRow) -> std::result::Result<PostSummary, sqlx::Error> {
    let Json(categories): Json<Vec<TaxonomyRow>> = row.try_get("categories")?;
    let categories = categories
        .into_iter()
        .map(|row| TaxonomyRef {
            id: row.id,
            slug: row.slug,
            name: row.name,
        })
        .collect();
    Ok(PostSummary {
        series: row.try_get::<Option<Json<_>>, _>("series")?.map(|j| j.0),
        id: row.try_get("id")?,
        slug: row.try_get("slug")?,
        slug_locked: row.try_get("slug_locked")?,
        title: row.try_get("title")?,
        categories,
        author: user(row.try_get("author_id")?, row.try_get("author_name")?),
        current_revision_id: row.try_get("current_revision_id")?,
        current_revision_no: row.try_get("revision_no")?,
        published_revision_id: row.try_get("published_revision_id")?,
        publication_version: row.try_get("publication_version")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn contains_pattern(keyword: &str) -> String {
    let mut pattern = String::with_capacity(keyword.len() + 2);
    pattern.push('%');
    for c in keyword.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// 锁定文章并返回 (当前编辑版本, 发布版本号)；文章不存在返回 `NotFound`。
async fn lock_post(connection: &mut PgConnection, id: Uuid) -> Result<(Uuid, i64)> {
    sqlx::query_as(
        "SELECT current_revision_id, publication_version FROM posts WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(connection)
    .await
    .map_err(db)?
    .ok_or(AppError::NotFound)
}

#[async_trait]
impl ContentRepository for PgContentRepository {
    #[tracing::instrument(name = "db.content_media_references", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn media_references(&self, ids: &[Uuid]) -> Result<Vec<MediaReference>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let _timer = METRICS.db_operation("content_media_references");
        let rows: Vec<(Uuid, String, Option<Uuid>, Option<i32>, Option<i32>)> = sqlx::query_as(
            "SELECT id, status, uploaded_by, width, height FROM media_assets WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&mut *acquire(&self.pool).await.map_err(db)?)
        .await
        .map_err(db)?;
        rows.into_iter()
            .map(|(id, status, uploaded_by, width, height)| {
                Ok(MediaReference {
                    id,
                    status: MediaStatus::parse(&status).ok_or_else(|| {
                        AppError::Internal(anyhow::anyhow!("unexpected media status {status}"))
                    })?,
                    uploaded_by,
                    width,
                    height,
                })
            })
            .collect()
    }

    #[tracing::instrument(name = "db.content_revision_media", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn revision_media(&self, revision_id: Uuid) -> Result<Vec<Uuid>> {
        let _timer = METRICS.db_operation("content_revision_media");
        sqlx::query_scalar(
            "SELECT media_id FROM post_revision_media WHERE revision_id=$1 \
             UNION SELECT cover_media_id FROM post_revisions WHERE id=$1 AND cover_media_id IS NOT NULL",
        )
        .bind(revision_id)
        .fetch_all(&mut *acquire(&self.pool).await.map_err(db)?)
        .await
        .map_err(db)
    }

    #[tracing::instrument(name = "db.content_missing_taxonomy", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn missing_taxonomy(
        &self,
        category_ids: &[Uuid],
        tag_ids: &[Uuid],
    ) -> Result<(Vec<Uuid>, Vec<Uuid>)> {
        let _timer = METRICS.db_operation("content_missing_taxonomy");
        let mut connection = acquire(&self.pool).await.map_err(db)?;
        let categories = sqlx::query_scalar(
            "SELECT id FROM unnest($1::uuid[]) AS ids(id) WHERE NOT EXISTS (SELECT 1 FROM categories c WHERE c.id=ids.id)",
        )
        .bind(category_ids)
        .fetch_all(&mut *connection)
        .await
        .map_err(db)?;
        let tags = sqlx::query_scalar(
            "SELECT id FROM unnest($1::uuid[]) AS ids(id) WHERE NOT EXISTS (SELECT 1 FROM tags t WHERE t.id=ids.id)",
        )
        .bind(tag_ids)
        .fetch_all(&mut *connection)
        .await
        .map_err(db)?;
        Ok((categories, tags))
    }

    #[tracing::instrument(name = "db.content_create_post", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn create_post(&self, new: NewPost, actor: &ContentActor<'_>) -> Result<Post> {
        let _timer = METRICS.db_operation("content_create_post");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        // 文章与首条修订互相引用；当前修订外键延迟到提交时检查。
        sqlx::query(
            "INSERT INTO posts (id, slug, current_revision_id, author_id) VALUES ($1,$2,$3,$4)",
        )
        .bind(new.id)
        .bind(&new.slug)
        .bind(new.revision.id)
        .bind(new.author_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        insert_revision(&mut tx, new.id, 1, &new.revision).await?;
        audit(
            &mut tx,
            actor,
            "post.create",
            "post",
            new.id,
            json!({"slug": new.slug, "revision_id": new.revision.id}),
        )
        .await?;
        let created = post(&mut tx, new.id).await?.ok_or(AppError::NotFound)?;
        tx.commit().await.map_err(db)?;
        Ok(created)
    }

    #[tracing::instrument(name = "db.content_delete_post", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn delete_post(
        &self,
        post_id: Uuid,
        expected_revision_id: Uuid,
        expected_version: i64,
        actor: &ContentActor<'_>,
    ) -> Result<()> {
        let _timer = METRICS.db_operation("content_delete_post");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (current, version) = lock_post(&mut tx, post_id).await?;
        if current != expected_revision_id {
            return Err(AppError::Conflict(Conflict::RevisionConflict));
        }
        if version != expected_version {
            return Err(AppError::Conflict(Conflict::PublicationConflict));
        }
        let published: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT FROM published_posts WHERE post_id=$1)")
                .bind(post_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if published {
            return Err(AppError::Conflict(Conflict::PostInUse));
        }
        let in_series: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM series_items WHERE post_id=$1)")
                .bind(post_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if in_series {
            return Err(AppError::Conflict(Conflict::PostAlreadyInSeries));
        }
        sqlx::query("DELETE FROM posts WHERE id=$1")
            .bind(post_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit(
            &mut tx,
            actor,
            "post.delete",
            "post",
            post_id,
            json!({"revision_id": current}),
        )
        .await?;
        tx.commit().await.map_err(db)
    }

    #[tracing::instrument(name = "db.content_post_access", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn post_access(&self, id: Uuid) -> Result<Option<PostAccess>> {
        let _timer = METRICS.db_operation("content_post_access");
        let row: Option<(Option<Uuid>, Uuid)> =
            sqlx::query_as("SELECT author_id, current_revision_id FROM posts WHERE id=$1")
                .bind(id)
                .fetch_optional(&mut *acquire(&self.pool).await.map_err(db)?)
                .await
                .map_err(db)?;
        Ok(row.map(|(author_id, current_revision_id)| PostAccess {
            id,
            author_id,
            current_revision_id,
        }))
    }

    #[tracing::instrument(name = "db.content_post", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn post(&self, id: Uuid) -> Result<Option<Post>> {
        let _timer = METRICS.db_operation("content_post");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let result = post(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }

    #[tracing::instrument(name = "db.content_list_posts", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn list_posts(
        &self,
        page: &PageRequest,
        filter: &PostFilter,
    ) -> Result<Page<PostSummary>> {
        let _timer = METRICS.db_operation("content_list_posts");
        let pattern = filter.q.as_deref().map(contains_pattern);
        let order = match filter.sort {
            PostSort::UpdatedDesc => "p.updated_at DESC, p.id DESC",
            PostSort::CreatedDesc => "p.created_at DESC, p.id DESC",
            PostSort::TitleAsc => "r.title ASC, p.id ASC",
            PostSort::TitleDesc => "r.title DESC, p.id DESC",
        };
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let total = sqlx::query_scalar(&format!("SELECT count(*) {POST_LIST_FROM} {POST_FILTER}"))
            .bind(&pattern)
            .bind(filter.author_id)
            .bind(filter.category_id)
            .bind(filter.tag_id)
            .bind(filter.published)
            .bind(filter.uncategorized)
            .bind(filter.series.series_id)
            .bind(filter.series.in_series)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let rows = sqlx::query(&format!(
            "SELECT {MEMBERSHIP} AS series,p.id, p.slug, (p.first_published_at IS NOT NULL) AS slug_locked, r.title, p.author_id, au.display_name AS author_name, \
             p.current_revision_id, r.revision_no, \
             pub.id AS published_revision_id, p.publication_version, p.created_at, p.updated_at, \
             COALESCE((SELECT json_agg(json_build_object('id', c.id, 'slug', c.slug, 'name', c.name) ORDER BY c.name, c.slug) \
               FROM post_revision_categories rc JOIN categories c ON c.id=rc.category_id WHERE rc.revision_id=r.id), '[]') AS categories \
             {POST_LIST_FROM} {POST_FILTER} ORDER BY {order} LIMIT $9 OFFSET $10"
        ))
        .bind(&pattern)
        .bind(filter.author_id)
        .bind(filter.category_id)
        .bind(filter.tag_id)
        .bind(filter.published)
        .bind(filter.uncategorized)
        .bind(filter.series.series_id)
        .bind(filter.series.in_series)
        .bind(i64::from(page.per_page))
        .bind(page.offset())
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        let items = rows
            .iter()
            .map(read_summary)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Page {
            items,
            pagination: page.metadata(total),
        })
    }

    #[tracing::instrument(name = "db.content_post_category_counts", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn post_category_counts(
        &self,
        author_id: Option<Uuid>,
        series: silva_domain::content::SeriesFilter,
    ) -> Result<PostCategoryCounts> {
        let _timer = METRICS.db_operation("content_post_category_counts");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        const ELIGIBLE: &str = "WITH eligible AS (SELECT p.current_revision_id FROM posts p LEFT JOIN series_items i ON i.post_id=p.id
            WHERE ($1::uuid IS NULL OR p.author_id=$1) AND ($2::uuid IS NULL OR i.series_id=$2)
            AND ($3::bool IS NULL OR (i.series_id IS NOT NULL)=$3))";
        let (total, uncategorized): (i64, i64) = sqlx::query_as(&format!(
            "{ELIGIBLE} SELECT count(*) AS total, \
             count(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM post_revision_categories rc WHERE rc.revision_id=p.current_revision_id)) AS uncategorized \
             FROM eligible p"
        ))
        .bind(author_id)
        .bind(series.series_id)
        .bind(series.in_series)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        let rows = sqlx::query(&format!(
            "{ELIGIBLE} SELECT c.id, c.slug, c.name, c.description, \
             (SELECT count(*) FROM eligible p JOIN post_revision_categories rc ON rc.revision_id=p.current_revision_id \
              WHERE rc.category_id=c.id) AS post_count \
             FROM categories c \
             ORDER BY c.name, c.slug, c.id",
        ))
        .bind(author_id)
        .bind(series.series_id)
        .bind(series.in_series)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        let categories = rows
            .iter()
            .map(|row| {
                Ok(PostCategoryCount {
                    id: row.try_get("id")?,
                    slug: row.try_get("slug")?,
                    name: row.try_get("name")?,
                    description: row.try_get("description")?,
                    post_count: row.try_get("post_count")?,
                })
            })
            .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(PostCategoryCounts {
            total,
            uncategorized,
            categories,
        })
    }

    #[tracing::instrument(name = "db.content_create_revision", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn create_revision(
        &self,
        post_id: Uuid,
        expected_revision_id: Uuid,
        new: NewRevision,
        actor: &ContentActor<'_>,
    ) -> Result<SavedRevision> {
        let _timer = METRICS.db_operation("content_create_revision");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (current, _) = lock_post(&mut tx, post_id).await?;
        if current != expected_revision_id {
            return Err(AppError::Conflict(Conflict::RevisionConflict));
        }
        if revision_content(&mut tx, current)
            .await?
            .same_as(&new.content)
        {
            let unchanged = revision(&mut tx, post_id, current)
                .await?
                .ok_or(AppError::NotFound)?;
            tx.commit().await.map_err(db)?;
            return Ok(SavedRevision::Unchanged(unchanged));
        }
        let revision_no: i32 = sqlx::query_scalar(
            "DELETE FROM post_revisions WHERE post_id=$1 RETURNING revision_no + 1",
        )
        .bind(post_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        insert_revision(&mut tx, post_id, revision_no, &new).await?;
        sqlx::query("UPDATE posts SET current_revision_id=$2 WHERE id=$1")
            .bind(post_id)
            .bind(new.id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit(
            &mut tx,
            actor,
            "post.revision_create",
            "post",
            post_id,
            json!({"revision_id": new.id, "revision_no": revision_no}),
        )
        .await?;
        let created = revision(&mut tx, post_id, new.id)
            .await?
            .ok_or(AppError::NotFound)?;
        tx.commit().await.map_err(db)?;
        Ok(SavedRevision::Created(created))
    }

    #[tracing::instrument(name = "db.content_update_slug", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn update_slug(
        &self,
        post_id: Uuid,
        slug: &str,
        actor: &ContentActor<'_>,
    ) -> Result<Post> {
        let _timer = METRICS.db_operation("content_update_slug");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let locked: bool = sqlx::query_scalar(
            "SELECT first_published_at IS NOT NULL FROM posts WHERE id=$1 FOR UPDATE",
        )
        .bind(post_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(AppError::NotFound)?;
        if locked {
            return Err(AppError::Conflict(Conflict::SlugLocked));
        }
        sqlx::query("UPDATE posts SET slug=$2 WHERE id=$1")
            .bind(post_id)
            .bind(slug)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit(
            &mut tx,
            actor,
            "post.slug_update",
            "post",
            post_id,
            json!({"slug": slug}),
        )
        .await?;
        let updated = post(&mut tx, post_id).await?.ok_or(AppError::NotFound)?;
        tx.commit().await.map_err(db)?;
        Ok(updated)
    }

    #[tracing::instrument(name = "db.content_revisions", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn revisions(&self, post_id: Uuid, page: &PageRequest) -> Result<Page<RevisionSummary>> {
        let _timer = METRICS.db_operation("content_revisions");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let total = sqlx::query_scalar("SELECT count(*) FROM post_revisions WHERE post_id=$1")
            .bind(post_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let rows = sqlx::query(
            "SELECT r.id, r.revision_no, r.title, r.created_by, cu.display_name AS created_by_name, r.created_at FROM post_revisions r \
             LEFT JOIN users cu ON cu.id=r.created_by \
             WHERE r.post_id=$1 ORDER BY r.revision_no DESC LIMIT $2 OFFSET $3",
        )
        .bind(post_id)
        .bind(i64::from(page.per_page))
        .bind(page.offset())
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        let items = rows
            .iter()
            .map(|row| {
                Ok(RevisionSummary {
                    id: row.try_get("id")?,
                    revision_no: row.try_get("revision_no")?,
                    title: row.try_get("title")?,
                    created_by: user(row.try_get("created_by")?, row.try_get("created_by_name")?),
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Page {
            items,
            pagination: page.metadata(total),
        })
    }

    #[tracing::instrument(name = "db.content_revision", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn revision(&self, post_id: Uuid, revision_id: Uuid) -> Result<Option<Revision>> {
        let _timer = METRICS.db_operation("content_revision");
        revision(
            &mut *acquire(&self.pool).await.map_err(db)?,
            post_id,
            revision_id,
        )
        .await
    }

    #[tracing::instrument(name = "db.content_publish", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn publish(
        &self,
        post_id: Uuid,
        revision_id: Uuid,
        expected_version: i64,
        actor: &ContentActor<'_>,
    ) -> Result<PublicationResult> {
        let _timer = METRICS.db_operation("content_publish");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (current, version) = lock_post(&mut tx, post_id).await?;
        if version != expected_version {
            return Err(AppError::Conflict(Conflict::PublicationConflict));
        }
        if current != revision_id {
            return Err(AppError::invalid(
                "revision_id",
                FieldErrorCode::InvalidValue,
                "the revision is not the current revision of this post",
            ));
        }
        sqlx::query("SELECT save_post_publication($1,$2)")
            .bind(post_id)
            .bind(revision_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let target = advance_generation(&mut tx).await?;
        let publication_version: i64 = sqlx::query_scalar(
            "UPDATE posts SET first_published_at=COALESCE(first_published_at, now()), \
             publication_version=publication_version + 1 WHERE id=$1 RETURNING publication_version",
        )
        .bind(post_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        audit(
            &mut tx,
            actor,
            "post.publish",
            "post",
            post_id,
            json!({"revision_id": revision_id, "generation": target.generation}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(PublicationResult {
            generation: target.generation,
            publication_version,
        })
    }

    #[tracing::instrument(name = "db.content_unpublish", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn unpublish(
        &self,
        post_id: Uuid,
        expected_version: i64,
        actor: &ContentActor<'_>,
    ) -> Result<PublicationResult> {
        let _timer = METRICS.db_operation("content_unpublish");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let (_, version) = lock_post(&mut tx, post_id).await?;
        if version != expected_version {
            return Err(AppError::Conflict(Conflict::PublicationConflict));
        }
        let deleted = sqlx::query("DELETE FROM published_posts WHERE post_id=$1")
            .bind(post_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if deleted.rows_affected() == 0 {
            return Err(AppError::Conflict(Conflict::AlreadyUnpublished));
        }
        let target = advance_generation(&mut tx).await?;
        let publication_version: i64 = sqlx::query_scalar(
            "UPDATE posts SET publication_version=publication_version + 1 WHERE id=$1 RETURNING publication_version",
        )
        .bind(post_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        audit(
            &mut tx,
            actor,
            "post.unpublish",
            "post",
            post_id,
            json!({"generation": target.generation}),
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(PublicationResult {
            generation: target.generation,
            publication_version,
        })
    }
}
