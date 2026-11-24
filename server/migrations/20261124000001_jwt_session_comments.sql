-- 后台认证改为 JWT access token + refresh token；会话行仍是撤销依据，只更新注释，不改结构。
COMMENT ON TABLE sessions IS '后台登录会话，每次登录创建一条；access token 与 refresh token 通过会话 ID 关联，删除该行即撤销其全部令牌。';
COMMENT ON COLUMN sessions.token_hash IS '当前有效 refresh token 的 SHA-256 哈希，每次刷新轮换；不保存令牌原文';
COMMENT ON COLUMN sessions.expires_at IS '当前 refresh token 的过期时间，即会话失效时间；每次刷新按 refresh token 有效期重新计算';
COMMENT ON COLUMN sessions.last_seen_at IS '最近一次记录的会话活动时间（鉴权最多每 5 分钟更新一次，刷新令牌时更新），未记录时为 NULL';
