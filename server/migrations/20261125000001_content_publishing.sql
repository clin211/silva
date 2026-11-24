-- 文章、修订、分类标签、审核与站点发布：补充时间与归属字段、线上版本映射、约束和查询索引。
-- 内容与发布用例此前未实现，相关表中没有数据。

-- 文章首次被接受发布的时间，即公开站点显示的发布时间。
ALTER TABLE posts ADD COLUMN first_published_at timestamptz;
ALTER TABLE posts
  ADD CONSTRAINT posts_slug_format_check
    CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 100),
  ADD CONSTRAINT posts_post_type_check CHECK (post_type = 'article'),
  ADD CONSTRAINT posts_publication_version_check CHECK (publication_version >= 0),
  -- 曾被接受发布的文章必须已锁定 slug。
  ADD CONSTRAINT posts_first_published_lock_check
    CHECK (first_published_at IS NULL OR slug_locked);
COMMENT ON COLUMN posts.first_published_at IS '文章首次被接受发布的时间，作为公开发布时间；从未发布时为 NULL，取消发布后保留';

-- 修订保存时由 Markdown 生成的 h2–h4 目录，与 rendered_html 中的标题 id 对应。
ALTER TABLE post_revisions
  ADD COLUMN rendered_headings jsonb NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE post_revisions
  ADD CONSTRAINT post_revisions_revision_no_check CHECK (revision_no > 0),
  ADD CONSTRAINT post_revisions_rendered_headings_check
    CHECK (jsonb_typeof(rendered_headings) = 'array');
COMMENT ON COLUMN post_revisions.rendered_headings IS '保存修订时由 Markdown 生成的 h2–h4 目录数组，元素含 depth、slug、text；与 rendered_html 的标题 id 一致';
COMMENT ON COLUMN post_revisions.rendered_html IS '保存修订时由 Markdown 渲染并消毒的 HTML，正文图片指向公开媒体地址；公开导出只使用该字段';

ALTER TABLE categories ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE tags ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE categories
  ADD CONSTRAINT categories_slug_format_check
    CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 100);
ALTER TABLE tags
  ADD CONSTRAINT tags_slug_format_check
    CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 100);
CREATE TRIGGER maintain_updated_at BEFORE UPDATE ON categories
  FOR EACH ROW EXECUTE FUNCTION maintain_updated_at();
CREATE TRIGGER maintain_updated_at BEFORE UPDATE ON tags
  FOR EACH ROW EXECUTE FUNCTION maintain_updated_at();
COMMENT ON COLUMN categories.updated_at IS '该记录在本系统最近一次更新的时间。';
COMMENT ON COLUMN tags.updated_at IS '该记录在本系统最近一次更新的时间。';

ALTER TABLE post_reviews
  ADD COLUMN submitted_by uuid REFERENCES users(id) ON DELETE NO ACTION;
ALTER TABLE post_reviews
  ADD CONSTRAINT post_reviews_decision_check
    CHECK ((status = 'pending') = (reviewed_at IS NULL)
      AND (status = 'pending' OR reviewer_id IS NOT NULL));
COMMENT ON COLUMN post_reviews.submitted_by IS '提交审核的用户 ID，无关联用户时为 NULL';

-- 每个已构建 generation 中公开的文章及其修订，供后台查询文章当前线上版本。
CREATE TABLE site_generation_posts (
  generation bigint NOT NULL REFERENCES publish_jobs(generation) ON DELETE CASCADE,
  post_id uuid NOT NULL,
  revision_id uuid NOT NULL,
  PRIMARY KEY (generation, post_id),
  FOREIGN KEY (post_id, revision_id) REFERENCES post_revisions(post_id, id) ON DELETE CASCADE
);
COMMENT ON TABLE site_generation_posts IS '某个站点 generation 中公开的文章与其公开修订，线上切换时与 active_generation 一同写入。';
COMMENT ON COLUMN site_generation_posts.generation IS '该映射所属的站点 generation';
COMMENT ON COLUMN site_generation_posts.post_id IS '在该 generation 中公开的文章 ID';
COMMENT ON COLUMN site_generation_posts.revision_id IS '该文章在该 generation 中公开的修订 ID';

-- 构建失败后重新排队的任务在该时间之前不被领取，避免短暂故障连续耗尽自动重试。
ALTER TABLE publish_jobs ADD COLUMN retry_after timestamptz;
COMMENT ON COLUMN publish_jobs.retry_after IS '失败后自动重试的最早领取时间；NULL 表示可以立即领取';

ALTER TABLE publish_jobs
  ADD CONSTRAINT publish_jobs_attempts_check CHECK (attempts >= 0),
  ADD CONSTRAINT publish_jobs_lease_check
    CHECK ((status = 'building') = (lease_token IS NOT NULL AND lease_expires_at IS NOT NULL));
COMMENT ON COLUMN publish_jobs.manifest_key IS '产物清单的对象存储键，形如 site/{generation}/{attempt}/manifest.json；上线前为 NULL';
COMMENT ON COLUMN publish_jobs.artifact_prefix IS '构建产物在对象存储中的前缀，形如 site/{generation}/{attempt}/dist/；上线前为 NULL';

CREATE INDEX posts_author_created_idx ON posts (author_id, created_at DESC, id DESC);
CREATE INDEX posts_created_idx ON posts (created_at DESC, id DESC);
CREATE INDEX posts_desired_revision_idx ON posts (desired_published_revision_id)
  WHERE desired_published_revision_id IS NOT NULL;
CREATE INDEX post_revision_categories_category_idx ON post_revision_categories (category_id);
CREATE INDEX post_revision_tags_tag_idx ON post_revision_tags (tag_id);
CREATE INDEX post_reviews_status_created_idx ON post_reviews (status, created_at, id);
CREATE INDEX publish_jobs_status_idx ON publish_jobs (status, generation);
CREATE INDEX site_generation_posts_post_idx ON site_generation_posts (post_id, generation);
