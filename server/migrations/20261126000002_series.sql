CREATE TABLE series (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  slug text NOT NULL UNIQUE
    CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 100),
  slug_locked boolean NOT NULL DEFAULT false,
  title text NOT NULL,
  description text,
  cover_media_id uuid REFERENCES media_assets(id) ON DELETE RESTRICT,
  category_id uuid REFERENCES categories(id) ON DELETE RESTRICT,
  completion_status text NOT NULL DEFAULT 'ongoing'
    CHECK (completion_status IN ('ongoing', 'completed')),
  is_featured boolean NOT NULL DEFAULT false,
  sort_order integer NOT NULL DEFAULT 0,
  seo_title text,
  seo_description text,
  desired_published_at timestamptz,
  first_published_at timestamptz,
  edit_version bigint NOT NULL DEFAULT 0 CHECK (edit_version >= 0),
  publication_version bigint NOT NULL DEFAULT 0 CHECK (publication_version >= 0),
  created_by uuid REFERENCES users(id) ON DELETE NO ACTION,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (first_published_at IS NULL OR slug_locked),
  CHECK (desired_published_at IS NULL OR first_published_at IS NOT NULL)
);

CREATE TABLE series_sections (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  series_id uuid NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  title text NOT NULL,
  description text,
  position integer NOT NULL CHECK (position >= 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (series_id, id),
  UNIQUE (series_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE series_items (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  series_id uuid NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  section_id uuid,
  post_id uuid NOT NULL UNIQUE REFERENCES posts(id) ON DELETE CASCADE,
  position integer NOT NULL CHECK (position >= 0),
  display_title text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE NULLS NOT DISTINCT (series_id, section_id, position) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (series_id, section_id)
    REFERENCES series_sections(series_id, id) ON DELETE NO ACTION
);

CREATE TABLE site_generation_series (
  generation bigint NOT NULL REFERENCES publish_jobs(generation) ON DELETE CASCADE,
  series_id uuid NOT NULL REFERENCES series(id) ON DELETE RESTRICT,
  edit_version bigint NOT NULL CHECK (edit_version >= 0),
  cover_media_id uuid REFERENCES media_assets(id) ON DELETE RESTRICT,
  PRIMARY KEY (generation, series_id)
);

COMMENT ON TABLE series IS '文章系列';
COMMENT ON COLUMN series.id IS '系列标识';
COMMENT ON COLUMN series.slug IS '固定链接';
COMMENT ON COLUMN series.slug_locked IS '首次公开后锁定链接';
COMMENT ON COLUMN series.title IS '系列标题';
COMMENT ON COLUMN series.description IS '系列简介';
COMMENT ON COLUMN series.cover_media_id IS '封面媒体';
COMMENT ON COLUMN series.category_id IS '主分类';
COMMENT ON COLUMN series.completion_status IS '连载状态';
COMMENT ON COLUMN series.is_featured IS '精选标记';
COMMENT ON COLUMN series.sort_order IS '概览排序权重';
COMMENT ON COLUMN series.seo_title IS '搜索引擎标题';
COMMENT ON COLUMN series.seo_description IS '搜索引擎摘要';
COMMENT ON COLUMN series.desired_published_at IS '期望公开时间';
COMMENT ON COLUMN series.first_published_at IS '首次受理公开时间';
COMMENT ON COLUMN series.edit_version IS '编辑版本';
COMMENT ON COLUMN series.publication_version IS '发布版本';
COMMENT ON COLUMN series.created_by IS '创建人';
COMMENT ON COLUMN series.created_at IS '创建时间';
COMMENT ON COLUMN series.updated_at IS '更新时间';
CREATE TRIGGER series_updated_at BEFORE UPDATE ON series
  FOR EACH ROW EXECUTE FUNCTION maintain_updated_at();

COMMENT ON TABLE series_sections IS '系列篇章';
COMMENT ON COLUMN series_sections.id IS '篇章标识';
COMMENT ON COLUMN series_sections.series_id IS '所属系列';
COMMENT ON COLUMN series_sections.title IS '篇章标题';
COMMENT ON COLUMN series_sections.description IS '篇章简介';
COMMENT ON COLUMN series_sections.position IS '篇章序号';
COMMENT ON COLUMN series_sections.created_at IS '创建时间';
COMMENT ON COLUMN series_sections.updated_at IS '更新时间';
CREATE TRIGGER series_sections_updated_at BEFORE UPDATE ON series_sections
  FOR EACH ROW EXECUTE FUNCTION maintain_updated_at();

COMMENT ON TABLE series_items IS '系列目录成员';
COMMENT ON COLUMN series_items.id IS '条目标识';
COMMENT ON COLUMN series_items.series_id IS '所属系列';
COMMENT ON COLUMN series_items.section_id IS '所属篇章（空表示根目录）';
COMMENT ON COLUMN series_items.post_id IS '文章标识';
COMMENT ON COLUMN series_items.position IS '组内序号';
COMMENT ON COLUMN series_items.display_title IS '目录短标题';
COMMENT ON COLUMN series_items.created_at IS '创建时间';
COMMENT ON COLUMN series_items.updated_at IS '更新时间';
CREATE TRIGGER series_items_updated_at BEFORE UPDATE ON series_items
  FOR EACH ROW EXECUTE FUNCTION maintain_updated_at();

COMMENT ON TABLE site_generation_series IS '当前线上系列版本与封面';
COMMENT ON COLUMN site_generation_series.generation IS '站点代号';
COMMENT ON COLUMN site_generation_series.series_id IS '系列标识';
COMMENT ON COLUMN site_generation_series.edit_version IS '构建时的编辑版本';
COMMENT ON COLUMN site_generation_series.cover_media_id IS '构建时的封面';

CREATE INDEX series_category_sort_idx ON series (category_id, sort_order ASC, created_at DESC);
CREATE INDEX series_desired_published_idx ON series (desired_published_at DESC) WHERE desired_published_at IS NOT NULL;
CREATE INDEX series_cover_media_idx ON series (cover_media_id) WHERE cover_media_id IS NOT NULL;
CREATE INDEX site_generation_series_series_idx ON site_generation_series (series_id, generation);
CREATE INDEX site_generation_series_cover_idx ON site_generation_series (cover_media_id) WHERE cover_media_id IS NOT NULL;

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
WHERE g.cover_media_id IS NOT NULL;
