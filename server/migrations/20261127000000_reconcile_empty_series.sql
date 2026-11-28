-- 旧构建器会为没有已发布成员的专栏也登记发布任务。该专栏在快照中过滤为空，
-- 即使构建失败，也可以沿用原线上版本，同时保留专栏的发布意图与失败记录。
DO $$
DECLARE
    live bigint;
    pending bigint;
    target_series uuid;
BEGIN
    -- 已经成功切换 SSR 的数据库不再有旧版映射表。
    IF EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = 20261127000001 AND success) THEN
        RETURN;
    END IF;
    SELECT active_generation, desired_generation INTO STRICT live, pending
    FROM site_state WHERE singleton FOR UPDATE;
    IF live IS NOT DISTINCT FROM pending THEN
        RETURN;
    END IF;

    -- 只处理线上版本之后唯一一次失败的发布，且审计必须能唯一定位到该专栏。
    SELECT (array_agg(a.resource_id))[1] INTO target_series
    FROM publish_jobs j JOIN audit_logs a
        ON a.metadata->'desired_generation' = to_jsonb(j.generation)
        AND a.action = 'series.publish' AND a.resource_type = 'series'
        AND a.actor_user_id IS NOT DISTINCT FROM j.requested_by
    WHERE j.generation = pending AND j.trigger_type = 'series.publish' AND j.status = 'failed'
        AND pending = live + 1
        AND EXISTS (SELECT 1 FROM publish_jobs WHERE generation = live AND status = 'active')
        AND NOT EXISTS (SELECT 1 FROM publish_jobs WHERE generation > pending)
    HAVING count(*) = 1;

    IF target_series IS NULL OR NOT EXISTS (
        SELECT 1 FROM series s WHERE s.id = target_series AND s.desired_published_at IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM site_generation_series WHERE generation = live AND series_id = s.id)
        AND NOT EXISTS (
            SELECT 1 FROM series_items i JOIN posts p ON p.id = i.post_id
            WHERE i.series_id = s.id AND p.desired_published_revision_id IS NOT NULL
        )
    ) THEN
        RAISE EXCEPTION 'SSR 切换已中止：线上版本 %，待发布版本 %。请核对尚未上线的变更。', live, pending;
    END IF;

    -- 比较旧站实际引用与 SSR 公开投影。任一侧缺失、增加或修改都保留旧状态。
    IF EXISTS (
        SELECT 1 FROM (SELECT post_id, revision_id FROM site_generation_posts WHERE generation = live) old
        FULL JOIN (SELECT id AS post_id, desired_published_revision_id AS revision_id FROM posts
                   WHERE desired_published_revision_id IS NOT NULL) current USING (post_id)
        WHERE old.revision_id IS DISTINCT FROM current.revision_id
    ) OR EXISTS (
        SELECT 1 FROM (SELECT series_id, edit_version, cover_media_id FROM site_generation_series WHERE generation = live) old
        FULL JOIN (SELECT s.id AS series_id, s.edit_version, s.cover_media_id FROM series s
                   WHERE s.desired_published_at IS NOT NULL AND EXISTS (
                       SELECT 1 FROM series_items i JOIN posts p ON p.id = i.post_id
                       WHERE i.series_id = s.id AND p.desired_published_revision_id IS NOT NULL
                   )) current USING (series_id)
        WHERE old.edit_version IS DISTINCT FROM current.edit_version
           OR old.cover_media_id IS DISTINCT FROM current.cover_media_id
    ) OR EXISTS (
        SELECT 1 FROM site_state WHERE active_avatar_media_id IS DISTINCT FROM
            (SELECT avatar_media_id FROM profiles ORDER BY updated_at DESC, id LIMIT 1)
    ) OR EXISTS (
        SELECT 1 FROM audit_logs a WHERE a.action = 'media.update' AND a.resource_type = 'media'
          AND a.created_at >= (SELECT coalesce(started_at, created_at) FROM publish_jobs WHERE generation = live)
          AND a.resource_id IN (
              SELECT media_id FROM live_post_media WHERE kind = 'post_cover'
              UNION ALL SELECT cover_media_id FROM site_generation_series WHERE generation = live
              UNION ALL SELECT active_avatar_media_id FROM site_state
          )
    ) THEN
        RAISE EXCEPTION 'SSR 切换已中止：公开引用与线上版本 % 不一致，请保留旧站并核对。', live;
    END IF;

    UPDATE site_state SET desired_generation = live WHERE singleton;
END $$;
