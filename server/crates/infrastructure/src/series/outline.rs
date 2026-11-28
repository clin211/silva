//! 全量目录按最终位置批量写入，未变更的条目保留标识和更新时间。
use super::*;
use serde::Serialize;
use silva_domain::series::{ItemInput, MAX_ITEMS, Outline};
use std::collections::HashMap;

#[derive(Serialize)]
struct SectionRow {
    id: Uuid,
    title: String,
    description: Option<String>,
    position: i32,
}

#[derive(Serialize)]
struct ItemRow {
    id: Uuid,
    post_id: Uuid,
    section_id: Option<Uuid>,
    display_title: Option<String>,
    position: i32,
}

async fn validate_posts(conn: &mut PgConnection, series: Uuid, posts: &[Uuid]) -> Result<()> {
    let owners: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT i.series_id FROM posts p LEFT JOIN series_items i ON i.post_id=p.id
         WHERE p.id=ANY($1) ORDER BY p.id FOR KEY SHARE OF p",
    )
    .bind(posts)
    .fetch_all(conn)
    .await
    .map_err(db)?;
    if owners.len() != posts.len() {
        return Err(AppError::NotFound);
    }
    if owners.into_iter().flatten().any(|id| id != series) {
        return Err(AppError::Conflict(Conflict::PostAlreadyInSeries));
    }
    Ok(())
}

pub(super) async fn replace(
    conn: &mut PgConnection,
    current: &Detail,
    input: Outline,
) -> Result<bool> {
    let id = current.summary.id;
    let existing: HashMap<_, _> = current.items().map(|item| (item.post_id, item)).collect();
    let mut sections = Vec::with_capacity(input.sections.len());
    let mut items = Vec::new();
    let mut items_changed = false;
    let mut add_group = |group: Vec<ItemInput>, section_id| {
        for (position, item) in group.into_iter().enumerate() {
            let position = position as i32;
            let post_id = item.post_id.as_uuid();
            let old = existing.get(&post_id);
            items_changed |= old.is_none_or(|old| {
                old.position != position
                    || old.section_id != section_id
                    || old.display_title != item.display_title
            });
            items.push(ItemRow {
                id: old.map(|i| i.id).unwrap_or_else(ids::new_id),
                post_id,
                section_id,
                display_title: item.display_title,
                position,
            });
        }
    };
    add_group(input.items, None);
    let mut changed = current.sections.len() != input.sections.len();
    for (position, section) in input.sections.into_iter().enumerate() {
        let section_id = match section.id {
            Some(s) => {
                let old = current
                    .sections
                    .iter()
                    .find(|old| old.id == s.as_uuid())
                    .ok_or_else(|| {
                        AppError::invalid(
                            format!("sections[{position}].id"),
                            FieldErrorCode::InvalidValue,
                            "section does not belong to this series",
                        )
                    })?;
                changed |= old.position != position as i32
                    || old.title != section.title
                    || old.description != section.description;
                old.id
            }
            None => {
                changed = true;
                ids::new_id()
            }
        };
        sections.push(SectionRow {
            id: section_id,
            position: position as i32,
            title: section.title,
            description: section.description,
        });
        add_group(section.items, Some(section_id));
    }
    if !changed && !items_changed && items.len() == existing.len() {
        return Ok(false);
    }
    let post_ids: Vec<_> = items.iter().map(|i| i.post_id).collect();
    validate_posts(conn, id, &post_ids).await?;
    sqlx::query("INSERT INTO series_sections AS s(id,series_id,title,description,position)
        SELECT id,$1,title,description,position FROM jsonb_to_recordset($2) AS x(id uuid,title text,description text,position int)
        ON CONFLICT(id) DO UPDATE SET title=excluded.title,description=excluded.description,position=excluded.position
        WHERE (s.title,s.description,s.position) IS DISTINCT FROM (excluded.title,excluded.description,excluded.position)")
        .bind(id)
        .bind(Json(&sections))
        .execute(&mut *conn)
        .await
        .map_err(write_error)?;
    sqlx::query("DELETE FROM series_items WHERE series_id=$1 AND NOT(post_id=ANY($2))")
        .bind(id)
        .bind(&post_ids)
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    sqlx::query("INSERT INTO series_items AS i(id,series_id,post_id,section_id,display_title,position)
        SELECT id,$1,post_id,section_id,display_title,position FROM jsonb_to_recordset($2)
        AS x(id uuid,post_id uuid,section_id uuid,display_title text,position int) ORDER BY post_id
        ON CONFLICT(id) DO UPDATE SET section_id=excluded.section_id,display_title=excluded.display_title,position=excluded.position
        WHERE (i.section_id,i.display_title,i.position) IS DISTINCT FROM (excluded.section_id,excluded.display_title,excluded.position)")
        .bind(id)
        .bind(Json(&items))
        .execute(&mut *conn)
        .await
        .map_err(write_error)?;
    let section_ids: Vec<_> = sections.iter().map(|s| s.id).collect();
    sqlx::query("DELETE FROM series_sections WHERE series_id=$1 AND NOT(id=ANY($2))")
        .bind(id)
        .bind(section_ids)
        .execute(conn)
        .await
        .map_err(db)?;
    Ok(true)
}

pub(super) async fn append(
    conn: &mut PgConnection,
    current: &Detail,
    section_id: Option<Uuid>,
    item: ItemInput,
) -> Result<()> {
    let post_id = item.post_id.as_uuid();
    let id = current.summary.id;
    let group = match section_id {
        Some(section) => {
            &current
                .sections
                .iter()
                .find(|s| s.id == section)
                .ok_or_else(|| {
                    AppError::invalid(
                        "section_id",
                        FieldErrorCode::InvalidValue,
                        "section does not belong to this series",
                    )
                })?
                .items
        }
        None => &current.items,
    };
    validate_posts(conn, id, &[post_id]).await?;
    if current.items().any(|i| i.post_id == post_id) {
        return Err(AppError::Conflict(Conflict::PostAlreadyInSeries));
    }
    if current.summary.item_count >= MAX_ITEMS as i64 {
        return Err(AppError::invalid(
            "post_id",
            FieldErrorCode::OutOfRange,
            "at most 200 posts",
        ));
    }
    let position = group.last().map_or(0, |i| i.position + 1);
    sqlx::query("INSERT INTO series_items(id,series_id,post_id,section_id,display_title,position) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(ids::new_id())
        .bind(id)
        .bind(post_id)
        .bind(section_id)
        .bind(item.display_title)
        .bind(position)
        .execute(conn).await.map_err(write_error)?;
    Ok(())
}

pub(super) async fn remove(conn: &mut PgConnection, id: Uuid, post: Uuid) -> Result<()> {
    let (section, position): (Option<Uuid>, i32) = sqlx::query_as(
        "DELETE FROM series_items WHERE series_id=$1 AND post_id=$2 RETURNING section_id,position",
    )
    .bind(id)
    .bind(post)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db)?
    .ok_or(AppError::NotFound)?;
    sqlx::query(
        "UPDATE series_items SET position=position-1
         WHERE series_id=$1 AND section_id IS NOT DISTINCT FROM $2 AND position>$3",
    )
    .bind(id)
    .bind(section)
    .bind(position)
    .execute(conn)
    .await
    .map_err(db)?;
    Ok(())
}
