-- 先按篇章分组过滤没有公开修订的文章，再按系列组装，空篇章与空系列自然消失。
WITH visible_groups AS (
    SELECT i.series_id, i.section_id, max(r.published_at) AS updated_at,
           jsonb_agg(jsonb_build_object('post_slug', p.slug, 'display_title', coalesce(i.display_title, r.title))
                     ORDER BY i.position, i.id) AS items
    FROM series_items i
    JOIN series s ON s.id=i.series_id AND s.published_at IS NOT NULL
    JOIN posts p ON p.id=i.post_id
    JOIN published_posts r ON r.post_id=p.id
    GROUP BY i.series_id, i.section_id
), outlines AS (
    SELECT g.series_id, max(g.updated_at) AS updated_at,
           coalesce((jsonb_agg(g.items) FILTER (WHERE g.section_id IS NULL))->0, '[]') AS items,
           coalesce(jsonb_agg(jsonb_build_object('title', sec.title, 'description', sec.description, 'items', g.items)
                     ORDER BY sec.position, sec.id) FILTER (WHERE sec.id IS NOT NULL), '[]') AS sections
    FROM visible_groups g LEFT JOIN series_sections sec ON sec.id=g.section_id
    GROUP BY g.series_id
)
SELECT jsonb_build_object(
    'id',s.id, 'slug',s.slug, 'title',s.title, 'description',s.description,
    'category',CASE WHEN c.id IS NOT NULL THEN jsonb_build_object('slug',c.slug,'name',c.name,'description',c.description) END,
    'completion_status',s.completion_status, 'featured',s.is_featured,
    'cover',CASE WHEN m.id IS NOT NULL THEN jsonb_build_object('media_id',m.id,'mime_type',m.mime_type,'width',m.width,'height',m.height,'alt',m.alt_text) END,
    'seo_title',s.seo_title, 'seo_description',s.seo_description, 'published_at',s.first_published_at,
    'updated_at',greatest(s.updated_at,o.updated_at), 'items',o.items, 'sections',o.sections)
FROM outlines o JOIN series s ON s.id=o.series_id
LEFT JOIN categories c ON c.id=s.category_id LEFT JOIN media_assets m ON m.id=s.cover_media_id
ORDER BY s.sort_order, s.first_published_at DESC, s.id
