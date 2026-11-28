-- 每篇文章只有一个可编辑修订。发布目标只存站点构建所需的字段。
LOCK TABLE posts, site_state IN SHARE ROW EXCLUSIVE MODE;

CREATE TABLE published_posts (
    id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
    post_id uuid NOT NULL UNIQUE REFERENCES posts(id) ON DELETE CASCADE,
    revision_no integer NOT NULL,
    title text NOT NULL,
    excerpt text,
    rendered_html text,
    rendered_headings jsonb NOT NULL,
    cover_media_id uuid REFERENCES media_assets(id) ON DELETE RESTRICT,
    is_featured boolean NOT NULL,
    seo_title text,
    seo_description text,
    UNIQUE(post_id, id)
);
COMMENT ON TABLE published_posts IS '最近一次显式发布所选的站点内容，每篇文章最多一份。';
COMMENT ON COLUMN published_posts.id IS '发布时的修订标识';
COMMENT ON COLUMN published_posts.post_id IS '文章标识';
COMMENT ON COLUMN published_posts.revision_no IS '发布时的保存序号';
COMMENT ON COLUMN published_posts.title IS '公开标题';
COMMENT ON COLUMN published_posts.excerpt IS '公开摘要';
COMMENT ON COLUMN published_posts.rendered_html IS '已消毒的公开正文';
COMMENT ON COLUMN published_posts.rendered_headings IS '公开目录';
COMMENT ON COLUMN published_posts.cover_media_id IS '封面媒体标识';
COMMENT ON COLUMN published_posts.is_featured IS '是否精选';
COMMENT ON COLUMN published_posts.seo_title IS '公开SEO标题';
COMMENT ON COLUMN published_posts.seo_description IS '公开SEO描述';

CREATE TABLE published_post_categories (
    revision_id uuid NOT NULL REFERENCES published_posts(id) ON DELETE CASCADE,
    category_id uuid NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    PRIMARY KEY (revision_id, category_id)
);
CREATE INDEX published_post_categories_ref_idx ON published_post_categories (category_id);
COMMENT ON TABLE published_post_categories IS '文章发布目标引用的分类';
COMMENT ON COLUMN published_post_categories.revision_id IS '发布时的修订标识';
COMMENT ON COLUMN published_post_categories.category_id IS '分类标识';

CREATE TABLE published_post_tags (
    revision_id uuid NOT NULL REFERENCES published_posts(id) ON DELETE CASCADE,
    tag_id uuid NOT NULL REFERENCES tags(id) ON DELETE RESTRICT,
    PRIMARY KEY (revision_id, tag_id)
);
CREATE INDEX published_post_tags_ref_idx ON published_post_tags (tag_id);
COMMENT ON TABLE published_post_tags IS '文章发布目标引用的标签';
COMMENT ON COLUMN published_post_tags.revision_id IS '发布时的修订标识';
COMMENT ON COLUMN published_post_tags.tag_id IS '标签标识';

CREATE TABLE published_post_media (
    revision_id uuid NOT NULL REFERENCES published_posts(id) ON DELETE CASCADE,
    media_id uuid NOT NULL REFERENCES media_assets(id) ON DELETE RESTRICT,
    PRIMARY KEY (revision_id, media_id)
);
CREATE INDEX published_post_media_ref_idx ON published_post_media (media_id);
COMMENT ON TABLE published_post_media IS '文章发布目标引用的正文媒体';
COMMENT ON COLUMN published_post_media.revision_id IS '发布时的修订标识';
COMMENT ON COLUMN published_post_media.media_id IS '正文媒体标识';

-- 在文章锁保护下替换发布目标；迁移同样使用此函数复制旧目标。
CREATE FUNCTION save_post_publication(target_post uuid, target_revision uuid) RETURNS void
LANGUAGE sql AS $$
    DELETE FROM published_posts WHERE post_id=target_post;
    INSERT INTO published_posts (id, post_id, revision_no, title, excerpt, rendered_html, rendered_headings, cover_media_id, is_featured, seo_title, seo_description)
    SELECT id, post_id, revision_no, title, excerpt, rendered_html, rendered_headings, cover_media_id, is_featured, seo_title, seo_description FROM post_revisions WHERE post_id=target_post AND id=target_revision;
    INSERT INTO published_post_categories (revision_id, category_id)
    SELECT revision_id, category_id FROM post_revision_categories WHERE revision_id=target_revision;
    INSERT INTO published_post_tags (revision_id, tag_id)
    SELECT revision_id, tag_id FROM post_revision_tags WHERE revision_id=target_revision;
    INSERT INTO published_post_media (revision_id, media_id)
    SELECT revision_id, media_id FROM post_revision_media WHERE revision_id=target_revision;
$$;

SELECT save_post_publication(id, desired_published_revision_id)
FROM posts WHERE desired_published_revision_id IS NOT NULL;
ALTER TABLE posts DROP CONSTRAINT posts_desired_revision_fk,
    ADD CONSTRAINT posts_desired_publication_fk FOREIGN KEY (id, desired_published_revision_id)
    REFERENCES published_posts(post_id, id) DEFERRABLE INITIALLY DEFERRED;

-- 已上线页面可以继续引用之前的图片，直到新站点构建成功。
CREATE TABLE live_post_media (
    post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    media_id uuid NOT NULL REFERENCES media_assets(id) ON DELETE RESTRICT,
    kind text NOT NULL CHECK (kind IN ('post_body','post_cover')),
    revision_id uuid NOT NULL,
    revision_no integer NOT NULL,
    PRIMARY KEY (post_id, media_id, kind)
);
CREATE INDEX live_post_media_ref_idx ON live_post_media(media_id);
COMMENT ON TABLE live_post_media IS '当前线上文章占用的媒体，随站点切换一起更新。';
COMMENT ON COLUMN live_post_media.post_id IS '线上文章标识';
COMMENT ON COLUMN live_post_media.media_id IS '线上图片标识';
COMMENT ON COLUMN live_post_media.kind IS '封面或正文图片';
COMMENT ON COLUMN live_post_media.revision_id IS '发布时的修订标识';
COMMENT ON COLUMN live_post_media.revision_no IS '发布时的保存序号';

INSERT INTO live_post_media(post_id, media_id, kind, revision_id, revision_no)
SELECT r.post_id, m.media_id, m.kind, r.id, r.revision_no
FROM site_state s JOIN site_generation_posts g ON g.generation=s.active_generation
JOIN post_revisions r ON r.id=g.revision_id
CROSS JOIN LATERAL (
    SELECT r.cover_media_id AS media_id, 'post_cover' AS kind WHERE r.cover_media_id IS NOT NULL
    UNION ALL SELECT pm.media_id, 'post_body' FROM post_revision_media pm WHERE pm.revision_id=r.id
) m;
ALTER TABLE site_generation_posts DROP CONSTRAINT site_generation_posts_post_id_revision_id_fkey,
    ADD FOREIGN KEY (post_id) REFERENCES posts(id) ON DELETE CASCADE;

CREATE VIEW published_post_media_references AS
SELECT r.cover_media_id AS media_id, 'post_cover'::text AS kind, r.post_id, r.id AS revision_id, r.revision_no
FROM published_posts r WHERE r.cover_media_id IS NOT NULL
UNION ALL
SELECT pm.media_id, 'post_body', r.post_id, r.id, r.revision_no
FROM published_post_media pm JOIN published_posts r ON r.id=pm.revision_id;

CREATE OR REPLACE VIEW media_references AS
SELECT r.cover_media_id AS media_id, 'post_cover'::text AS kind, r.post_id, r.id AS revision_id,
       r.revision_no, NULL::uuid AS profile_id, NULL::uuid AS series_id
FROM post_revisions r WHERE r.cover_media_id IS NOT NULL
UNION ALL
SELECT m.media_id, 'post_body', r.post_id, r.id, r.revision_no, NULL, NULL
FROM post_revision_media m JOIN post_revisions r ON r.id = m.revision_id
UNION ALL
SELECT avatar_media_id, 'profile_avatar', NULL, NULL, NULL, id, NULL FROM profiles WHERE avatar_media_id IS NOT NULL
UNION ALL
SELECT active_avatar_media_id, 'live_avatar', NULL, NULL, NULL, NULL, NULL FROM site_state WHERE active_avatar_media_id IS NOT NULL
UNION ALL
SELECT cover_media_id, 'series_cover', NULL, NULL, NULL, NULL, id
FROM series WHERE cover_media_id IS NOT NULL
UNION ALL
SELECT g.cover_media_id, 'live_series_cover', NULL, NULL, NULL, NULL, g.series_id
FROM site_generation_series g JOIN site_state s ON s.active_generation=g.generation
WHERE g.cover_media_id IS NOT NULL
UNION
SELECT media_id, kind, post_id, revision_id, revision_no, NULL, NULL FROM published_post_media_references
UNION
SELECT media_id, kind, post_id, revision_id, revision_no, NULL, NULL FROM live_post_media;

DROP TABLE post_reviews;
DELETE FROM permissions WHERE key IN ('posts.review','posts.submit_own');
DELETE FROM post_revisions r USING posts p WHERE p.id=r.post_id AND r.id<>p.current_revision_id;
SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE post_revisions ADD UNIQUE(post_id);
COMMENT ON TABLE post_revisions IS '文章当前可编辑内容，每篇最多一份，保存时原子替换。';
