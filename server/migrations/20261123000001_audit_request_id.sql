-- 审计记录关联产生它的 HTTP 请求，并为后台审计筛选补充索引。
ALTER TABLE audit_logs
  ADD COLUMN request_id uuid CHECK (request_id IS NULL OR is_uuid_v7(request_id));
COMMENT ON COLUMN audit_logs.request_id IS '产生该记录的 HTTP 请求 UUIDv7，与响应头 X-Request-Id 一致；CLI 等非 HTTP 操作为 NULL';

CREATE INDEX audit_logs_actor_created_idx
  ON audit_logs(actor_user_id, created_at DESC, id DESC);
COMMENT ON INDEX audit_logs_actor_created_idx IS '按操作账户筛选审计记录并保持时间倒序分页';

CREATE INDEX audit_logs_resource_created_idx
  ON audit_logs(resource_id, created_at DESC, id DESC);
COMMENT ON INDEX audit_logs_resource_created_idx IS '按目标实体筛选审计记录并保持时间倒序分页';

CREATE INDEX audit_logs_request_id_idx
  ON audit_logs(request_id) WHERE request_id IS NOT NULL;
COMMENT ON INDEX audit_logs_request_id_idx IS '按 HTTP 请求 ID 定位审计记录';

CREATE INDEX audit_logs_action_pattern_idx
  ON audit_logs(action text_pattern_ops);
COMMENT ON INDEX audit_logs_action_pattern_idx IS '支持审计操作类型的精确匹配与前缀匹配';
