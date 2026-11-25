-- 个人资料可以整体替换。线上头像在下一版激活前仍需由公开媒体接口提供。
ALTER TABLE site_state ADD COLUMN active_avatar_media_id uuid REFERENCES media_assets(id);
COMMENT ON COLUMN site_state.active_avatar_media_id IS '当前线上版本的头像媒体；与 active_generation 同时切换，保护仍在使用的图片。';

UPDATE site_state SET active_avatar_media_id = (
    SELECT p.avatar_media_id FROM profiles p ORDER BY p.updated_at DESC, p.id LIMIT 1
) WHERE active_generation IS NOT NULL;
