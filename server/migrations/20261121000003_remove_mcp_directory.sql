-- 移除已取消的目录功能，保留已执行迁移的版本与校验和。
-- 按依赖顺序删除关联、版本及目录表，不级联删除其他业务对象。
DROP TABLE mcp_server_categories;
DROP TABLE mcp_server_versions;
DROP TABLE mcp_categories;
DROP TABLE mcp_servers;
