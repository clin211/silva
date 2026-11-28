import type { Schemas } from '@silva/api-client';

export type UserInfo = Schemas['UserInfo'];
export type RoleKey = Schemas['RoleKey'];

export const PERMISSIONS = {
    POSTS_CREATE: 'posts.create',
    POSTS_READ_OWN: 'posts.read_own',
    POSTS_EDIT_OWN: 'posts.edit_own',
    POSTS_MANAGE: 'posts.manage',
    POSTS_PUBLISH: 'posts.publish',
    SERIES_MANAGE: 'series.manage',
    TAXONOMY_MANAGE: 'taxonomy.manage',
    MEDIA_MANAGE_OWN: 'media.manage_own',
    MEDIA_MANAGE: 'media.manage',
    PROFILE_MANAGE: 'profile.manage',
    USERS_MANAGE: 'users.manage',
    ROLES_READ: 'roles.read',
    AUDIT_READ: 'audit.read',
} as const;

export type PermissionKey = (typeof PERMISSIONS)[keyof typeof PERMISSIONS];

export function hasPermission(user: UserInfo | null | undefined, permission: string): boolean {
    if (!user) return false;
    return user.permissions.includes(permission);
}

export function hasAnyPermission(
    user: UserInfo | null | undefined,
    permissions: readonly string[],
): boolean {
    if (!user) return false;
    if (permissions.length === 0) return true;
    return permissions.some((permission) => user.permissions.includes(permission));
}

export function hasRole(user: UserInfo | null | undefined, role: RoleKey): boolean {
    if (!user) return false;
    return user.roles.includes(role);
}

export function getPrimaryRoleLabel(roles: readonly RoleKey[] | undefined): string {
    if (!roles || roles.length === 0) return '成员';
    if (roles.includes('admin')) return 'Admin';
    if (roles.includes('editor')) return 'Editor';
    if (roles.includes('author')) return 'Author';
    return roles[0] ?? '成员';
}

export const ROLE_DESCRIPTIONS: Record<RoleKey, { title: string; summary: string }> = {
    author: {
        title: 'Author',
        summary: '创建并编辑自己的文章与修订、上传并引用自己的媒体。',
    },
    editor: {
        title: 'Editor',
        summary: '管理所有文章、系列与媒体、发布或取消发布、维护分类标签与个人资料。',
    },
    admin: {
        title: 'Admin',
        summary: '包含 Editor 全部权限，另可管理账户、刷新公开内容与查看审计和系统状态。',
    },
};

export const PERMISSION_LABELS: Record<string, string> = {
    'posts.create': '创建文章',
    'posts.read_own': '查看本人文章与分类标签',
    'posts.edit_own': '编辑本人文章与保存修订',
    'posts.manage': '管理全部文章',
    'posts.publish': '发布与取消发布文章及系列',
    'series.manage': '创建、编辑、删除文章系列与编排目录',
    'taxonomy.manage': '维护分类与标签',
    'media.manage_own': '上传并管理本人媒体',
    'media.manage': '管理全部媒体',
    'profile.manage': '维护个人资料',
    'users.manage': '管理后台账户与分配角色',
    'roles.read': '查看内置角色与权限',
    'audit.read': '查看审计日志',
    'system.read': '查看系统健康状态与运行指标',
    'resources.manage': '管理平台扩展资源配置',
    'ai.ingest': '触发内容索引与 AI 知识入库',
    'ai.manage': '管理 AI 能力配置与检索策略',
};
