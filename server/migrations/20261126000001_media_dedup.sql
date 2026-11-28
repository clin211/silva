-- 同一摘要只对应一个私有对象。回收后的再次上传取得新键，旧删除任务可以安全重试。
CREATE TABLE media_blobs (
    sha256 text PRIMARY KEY CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    storage_key text NOT NULL UNIQUE,
    stored boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (sha256, storage_key)
);
COMMENT ON TABLE media_blobs IS '由 SHA-256 唯一标识的共享图片文件；写对象前先登记以支持故障恢复';
COMMENT ON COLUMN media_blobs.sha256 IS '校验后的原始文件字节的 SHA-256 十六进制摘要';
COMMENT ON COLUMN media_blobs.storage_key IS '该次分配的私有对象键，回收后不再使用';
COMMENT ON COLUMN media_blobs.stored IS '正式对象已写入并提交';
COMMENT ON COLUMN media_blobs.created_at IS '对象键首次分配时间';

ALTER TABLE media_assets DROP CONSTRAINT media_assets_storage_key_key,
    ADD COLUMN blob_sha256 text,
    ADD FOREIGN KEY (blob_sha256, storage_key) REFERENCES media_blobs(sha256, storage_key),
    ADD CHECK (blob_sha256 IS NULL OR status = 'ready');
COMMENT ON COLUMN media_assets.blob_sha256 IS '已归并的图片内容摘要，旧媒体由后台分批补齐';
COMMENT ON COLUMN media_assets.storage_key IS '读取文件使用的实际对象键，可由多个媒体条目共享';
CREATE INDEX media_assets_blob_owner_idx ON media_assets(blob_sha256, uploaded_by, id)
    WHERE blob_sha256 IS NOT NULL;
CREATE INDEX media_assets_backfill_idx ON media_assets(purge_attempted_at NULLS FIRST, id)
    WHERE status = 'ready' AND blob_sha256 IS NULL;

CREATE TABLE media_object_deletions (
    storage_key text PRIMARY KEY,
    retry_at timestamptz NOT NULL DEFAULT now()
);
COMMENT ON TABLE media_object_deletions IS '已退出使用的对象键；后台成功删除对象后移除任务';
COMMENT ON COLUMN media_object_deletions.storage_key IS '可幂等删除且不会被重新分配的键';
COMMENT ON COLUMN media_object_deletions.retry_at IS '允许开始或再次尝试删除的时间';
CREATE INDEX media_object_deletions_due_idx ON media_object_deletions(retry_at);

CREATE VIEW media_references AS
SELECT r.cover_media_id AS media_id, 'post_cover'::text AS kind, r.post_id, r.id AS revision_id,
       r.revision_no, NULL::uuid AS profile_id
FROM post_revisions r WHERE r.cover_media_id IS NOT NULL
UNION ALL
SELECT m.media_id, 'post_body', r.post_id, r.id, r.revision_no, NULL
FROM post_revision_media m JOIN post_revisions r ON r.id = m.revision_id
UNION ALL
SELECT avatar_media_id, 'profile_avatar', NULL, NULL, NULL, id FROM profiles WHERE avatar_media_id IS NOT NULL
UNION ALL
SELECT active_avatar_media_id, 'live_avatar', NULL, NULL, NULL, NULL FROM site_state WHERE active_avatar_media_id IS NOT NULL;

-- 旧记录可能留下已写入但未确认的对象。记录删除与回收登记原子提交。
CREATE FUNCTION retire_legacy_media() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO media_object_deletions(storage_key)
    VALUES (OLD.upload_key), (COALESCE(OLD.storage_key, 'media/' || OLD.id::text))
    ON CONFLICT DO NOTHING;
    RETURN OLD;
END;
$$;
CREATE TRIGGER retire_legacy_media AFTER DELETE ON media_assets
    FOR EACH ROW WHEN (OLD.blob_sha256 IS NULL) EXECUTE FUNCTION retire_legacy_media();
CREATE UNIQUE INDEX media_assets_legacy_key_idx ON media_assets(storage_key)
    WHERE blob_sha256 IS NULL AND storage_key IS NOT NULL;
