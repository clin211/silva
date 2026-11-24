-- 媒体直传与确认：记录客户端声明、拒绝原因与时间和更新时间，并补充状态一致性约束与查询索引。
-- 媒体用例此前未实现，media_assets 中没有数据；新增的 NOT NULL 列不设默认值，
-- 若已有手工写入的记录，迁移会失败而不是填入虚构的声明值。
ALTER TABLE media_assets
  ADD COLUMN declared_mime_type text NOT NULL,
  ADD COLUMN declared_size_bytes bigint NOT NULL,
  ADD COLUMN rejection_reason text,
  ADD COLUMN rejected_at timestamptz,
  ADD COLUMN purge_attempted_at timestamptz,
  ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();

ALTER TABLE media_assets
  ADD CONSTRAINT media_assets_declared_mime_type_check
    CHECK (declared_mime_type IN ('image/jpeg', 'image/png', 'image/webp')),
  ADD CONSTRAINT media_assets_declared_size_bytes_check
    CHECK (declared_size_bytes BETWEEN 1 AND 10485760),
  ADD CONSTRAINT media_assets_mime_type_check
    CHECK (mime_type IS NULL OR mime_type IN ('image/jpeg', 'image/png', 'image/webp')),
  ADD CONSTRAINT media_assets_size_bytes_limit_check
    CHECK (size_bytes IS NULL OR size_bytes <= 10485760),
  ADD CONSTRAINT media_assets_dimensions_check
    CHECK ((width IS NULL OR width > 0) AND (height IS NULL OR height > 0)),
  ADD CONSTRAINT media_assets_ready_dimensions_check
    CHECK (status <> 'ready' OR (width IS NOT NULL AND height IS NOT NULL)),
  ADD CONSTRAINT media_assets_rejection_reason_check
    CHECK (
      (status = 'rejected') = (rejection_reason IS NOT NULL)
      AND (status = 'rejected') = (rejected_at IS NOT NULL)
      AND (rejection_reason IS NULL OR rejection_reason IN
        ('too_large', 'unsupported_format', 'undecodable', 'dimensions_exceeded'))
    );

CREATE TRIGGER maintain_updated_at BEFORE UPDATE ON media_assets
  FOR EACH ROW EXECUTE FUNCTION maintain_updated_at();

-- 作者列表按上传者筛选；Worker 按状态与截止时间查找可清理记录。
CREATE INDEX media_assets_uploaded_by_created_idx
  ON media_assets (uploaded_by, created_at DESC, id DESC);
CREATE INDEX media_assets_created_idx ON media_assets (created_at DESC, id DESC);
CREATE INDEX media_assets_pending_expiry_idx ON media_assets (upload_expires_at)
  WHERE status = 'pending';
CREATE INDEX media_assets_rejected_at_idx ON media_assets (rejected_at)
  WHERE status = 'rejected';
-- 删除媒体前检查引用；外键列默认没有索引。
CREATE INDEX post_revisions_cover_media_id_idx ON post_revisions (cover_media_id)
  WHERE cover_media_id IS NOT NULL;
CREATE INDEX post_revision_media_media_id_idx ON post_revision_media (media_id);
CREATE INDEX profiles_avatar_media_id_idx ON profiles (avatar_media_id)
  WHERE avatar_media_id IS NOT NULL;

COMMENT ON COLUMN media_assets.declared_mime_type IS '申请上传时客户端声明的 MIME 类型，仅作参考；实际类型以服务端识别的 mime_type 为准';
COMMENT ON COLUMN media_assets.declared_size_bytes IS '申请上传时客户端声明的文件字节数，范围 1 至 10485760';
COMMENT ON COLUMN media_assets.rejection_reason IS '校验拒绝原因：too_large、unsupported_format、undecodable、dimensions_exceeded；仅 rejected 状态有值，其余为 NULL';
COMMENT ON COLUMN media_assets.rejected_at IS '记录被拒绝的时间，用于计算已拒绝记录的保留期；仅 rejected 状态有值，其余为 NULL';
COMMENT ON COLUMN media_assets.purge_attempted_at IS 'Worker 最近一次清理该记录失败的时间，失败的记录排到后续批次末尾；从未失败时为 NULL';
COMMENT ON COLUMN media_assets.updated_at IS '该记录在本系统最近一次更新的时间。';
COMMENT ON COLUMN media_assets.status IS '媒体状态：pending 已申请直传待确认、ready 已校验可引用、rejected 未通过校验需重新申请';
COMMENT ON COLUMN media_assets.upload_key IS '浏览器预签名直传使用的私有临时对象键，形如 uploads/{id}；确认或清理后对象被删除';
COMMENT ON COLUMN media_assets.storage_key IS '校验后写入的不可覆盖私有正式对象键，形如 media/{id}；未通过校验时为 NULL';
