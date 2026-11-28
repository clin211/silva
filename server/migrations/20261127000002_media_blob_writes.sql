CREATE TABLE media_blob_writes (
    id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
    sha256 text NOT NULL REFERENCES media_blobs(sha256) ON DELETE RESTRICT,
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '1 hour')
);
COMMENT ON TABLE media_blob_writes IS '进行中的一次媒体写入，确认时消费，进程中断后由维护任务回收';
COMMENT ON COLUMN media_blob_writes.id IS '本次写入的 UUIDv7 凭证';
COMMENT ON COLUMN media_blob_writes.sha256 IS '凭证保护的共享文件摘要';
COMMENT ON COLUMN media_blob_writes.expires_at IS '维护任务可回收凭证的时间，晚于对象请求截止时间';
CREATE INDEX media_blob_writes_digest_idx ON media_blob_writes(sha256);
CREATE INDEX media_blob_writes_expiry_idx ON media_blob_writes(expires_at);
