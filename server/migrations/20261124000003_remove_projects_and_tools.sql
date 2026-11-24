-- 移除已取消的项目展览与工具导航，保留已执行迁移的版本与校验和。
-- 两者对媒体的引用（projects.cover_media_id、tools.icon_media_id）随表一并删除。
-- 按依赖顺序先删项目的子表，不级联删除其他业务对象。
DROP TABLE project_technologies;
DROP TABLE project_links;
DROP TABLE projects;
DROP TABLE tools;
