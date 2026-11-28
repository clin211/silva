-- 内容公开后在同一事务更新缓存版本，SSR 直接读取当前公开副本。
LOCK TABLE site_state, publish_jobs, posts, published_posts IN SHARE ROW EXCLUSIVE MODE;
-- 合并执行旧库升级时，先检查前序迁移留在外层事务中的延迟外键。
SET CONSTRAINTS ALL IMMEDIATE;

ALTER TABLE site_state ADD COLUMN generation bigint NOT NULL DEFAULT 0 CHECK (generation >= 0),
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT 'epoch';
UPDATE site_state SET generation=coalesce(active_generation,0), updated_at=coalesce(activated_at,'epoch');
ALTER TABLE site_state DROP COLUMN desired_generation, DROP COLUMN active_generation, DROP COLUMN activated_at;
COMMENT ON TABLE site_state IS '公开内容缓存版本，在公开数据的同一写事务内推进。';
COMMENT ON COLUMN site_state.generation IS '当前公开内容版本，空站点从零开始';
COMMENT ON COLUMN site_state.updated_at IS '当前版本生效时间';

ALTER TABLE series RENAME COLUMN desired_published_at TO published_at;
COMMENT ON COLUMN series.published_at IS '系列设置为公开的时间，撤回后为 NULL';

DELETE FROM published_posts r WHERE NOT EXISTS (
    SELECT FROM posts p WHERE p.id=r.post_id AND p.desired_published_revision_id=r.id
);
ALTER TABLE published_posts ADD COLUMN published_at timestamptz;
UPDATE published_posts r SET published_at=p.desired_published_at FROM posts p WHERE p.id=r.post_id;
ALTER TABLE published_posts ALTER COLUMN published_at SET NOT NULL,
    ALTER COLUMN published_at SET DEFAULT now();
COMMENT ON COLUMN published_posts.published_at IS '当前公开修订的发布时间';
ALTER TABLE posts DROP COLUMN desired_published_revision_id, DROP COLUMN desired_published_at, DROP COLUMN post_type, DROP COLUMN slug_locked;
ALTER TABLE series DROP COLUMN slug_locked;
-- 每篇文章只有一个草稿，post_id 的唯一约束已覆盖旧修订序号索引。
ALTER TABLE post_revisions DROP CONSTRAINT post_revisions_post_id_revision_no_key;
COMMENT ON TABLE posts IS '文章、当前草稿及首发信息；首次公开后锁定 slug。';
ALTER TABLE published_posts DROP CONSTRAINT published_posts_post_id_id_key;

-- 旧任务、投递事件和仅指向已删除任务的日志没有业务消费者。
DROP TABLE publish_jobs, outbox_events;
DELETE FROM permissions WHERE key IN ('jobs.read','jobs.read_own','publishing.retry');
DELETE FROM audit_logs WHERE resource_type='publish_job';
UPDATE audit_logs SET metadata=metadata - ARRAY['publish_job_id','desired_generation']
WHERE metadata ?| ARRAY['publish_job_id','desired_generation'];
