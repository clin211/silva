-- 在旧版仍能读取文章、审核和权限时检查待发布目标。锁一直持有到升级事务结束。
-- 已有系列模型的中间版本由后续迁移核对空系列及其线上映射。
DO $$
BEGIN
    IF to_regclass('public.series') IS NULL THEN
        PERFORM singleton FROM site_state WHERE singleton FOR UPDATE;
        IF EXISTS (SELECT FROM site_state WHERE desired_generation IS DISTINCT FROM active_generation) THEN
            RAISE EXCEPTION 'SSR 切换已中止：请先用旧服务核对并处理待发布目标。';
        END IF;
    END IF;
END $$;
