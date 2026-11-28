-- 只有线上内容和最后一次发布目标一致时，才能用数据库公开副本替代旧快照。
-- 约束也阻止在切换以后由仍运行的旧二进制写入异步目标。
LOCK TABLE site_state IN ACCESS EXCLUSIVE MODE;
DO $$
BEGIN
    IF EXISTS(SELECT 1 FROM site_state WHERE desired_generation IS DISTINCT FROM active_generation) THEN
        RAISE EXCEPTION 'SSR 切换已中止：存在尚未上线的旧发布目标，当前线上版本和待处理修改均已保留'
          USING HINT = '保留旧版本服务。由编辑核对待发布内容，决定完成或撤销这些改动，待线上版本等于最新目标后重新运行迁移。';
    END IF;
END $$;
ALTER TABLE site_state ADD CONSTRAINT site_publication_is_synchronous
    CHECK (desired_generation IS NOT DISTINCT FROM active_generation);

-- 已被线上版本取代的排队或构建记录不再有消费者，保留记录并结束其租约。
UPDATE publish_jobs SET status='superseded', lease_token=NULL, lease_expires_at=NULL, finished_at=now()
WHERE status IN ('queued','building') AND generation < (SELECT active_generation FROM site_state);
UPDATE outbox_events SET processed_at=now()
WHERE aggregate_type='site' AND event_type='site.publish_requested' AND processed_at IS NULL;

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
SELECT cover_media_id, 'series_cover', NULL, NULL, NULL, NULL, id
FROM series WHERE cover_media_id IS NOT NULL
UNION
SELECT media_id, kind, post_id, revision_id, revision_no, NULL, NULL FROM published_post_media_references;

-- 保留发布记录、草稿和发布副本；旧静态站点独占的派生引用已由上述视图替代。
DROP TABLE site_generation_series, site_generation_posts, live_post_media;
ALTER TABLE site_state DROP COLUMN active_avatar_media_id;
