import type { Schemas } from '@silva/api-client';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Check, Feather, Minus, PenTool, ShieldCheck, Users } from 'lucide-react';
import { Fragment, useMemo, type ComponentType } from 'react';
import { ErrorBanner, LoadingCard, PageContainer, PageHeader } from '@/components/ui/feedback';
import { apiClient, unwrapResponse } from '@/lib/api';
import { cn } from '@/lib/cn';
import { PERMISSION_LABELS, ROLE_DESCRIPTIONS, type RoleKey } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

const ROLE_ORDER: Record<RoleKey, number> = {
    author: 0,
    editor: 1,
    admin: 2,
};

const ROLE_VISUALS: Record<
    RoleKey,
    {
        icon: ComponentType<{ className?: string }>;
        iconBg: string;
        iconColor: string;
        barColor: string;
        highlights: string[];
    }
> = {
    author: {
        icon: Feather,
        iconBg: 'bg-[#EEF6F2]',
        iconColor: 'text-[#2C6E51]',
        barColor: 'bg-[#52B086]',
        highlights: ['创建与编辑本人文章及修订', '上传并管理本人媒体资源'],
    },
    editor: {
        icon: PenTool,
        iconBg: 'bg-accent-tint',
        iconColor: 'text-accent',
        barColor: 'bg-accent',
        highlights: ['管理全部文章与媒体资源', '发布与取消发布', '维护分类标签与个人资料'],
    },
    admin: {
        icon: ShieldCheck,
        iconBg: 'bg-rail',
        iconColor: 'text-[#6EE7B7]',
        barColor: 'bg-rail',
        highlights: [
            '包含 Editor 全部内容与发布权限',
            '管理后台账户并分配内置角色',
            '刷新公开内容、审计与系统健康',
        ],
    },
};

function getPermissionGroup(perm: string): string {
    if (perm.startsWith('posts.')) return '文章';
    if (perm.startsWith('media.') || perm.startsWith('taxonomy.') || perm.startsWith('profile.')) {
        return '分类、媒体与资料';
    }
    return '账户与系统安全';
}

const GROUP_ORDER = ['文章', '分类、媒体与资料', '账户与系统安全'];

export function RolesPage() {
    const rolesQuery = useQuery({
        queryKey: queryKeys.roles.list,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/roles')),
    });

    const roles = useMemo<Schemas['Role'][]>(() => {
        const list = rolesQuery.data?.data ?? [];
        return [...list].sort((a, b) => ROLE_ORDER[a.key] - ROLE_ORDER[b.key]);
    }, [rolesQuery.data]);

    const allPermissions = useMemo(() => {
        const set = new Set<string>();
        for (const role of roles) {
            for (const perm of role.permissions) {
                set.add(perm);
            }
        }
        return Array.from(set).sort();
    }, [roles]);

    const groupedPermissions = useMemo(() => {
        const map = new Map<string, string[]>();
        for (const group of GROUP_ORDER) {
            map.set(group, []);
        }
        for (const perm of allPermissions) {
            const g = getPermissionGroup(perm);
            const bucket = map.get(g) ?? [];
            bucket.push(perm);
            map.set(g, bucket);
        }
        return Array.from(map.entries()).filter(([, perms]) => perms.length > 0);
    }, [allPermissions]);

    if (rolesQuery.isLoading) {
        return (
            <PageContainer>
                <LoadingCard text="正在加载内置角色与权限…" />
            </PageContainer>
        );
    }

    if (rolesQuery.isError) {
        return (
            <PageContainer>
                <ErrorBanner
                    title="角色加载失败"
                    error={rolesQuery.error}
                    onRetry={() => void rolesQuery.refetch()}
                />
            </PageContainer>
        );
    }

    const totalPermCount = Math.max(1, allPermissions.length);

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="ACCESS CONTROL"
                title="角色与权限"
                description="系统内置 Author、Editor 与 Admin 三级角色，权限定义由服务端只读维护。如需为成员分配角色请前往账户管理。"
                actions={
                    <Link
                        to="/users"
                        className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-4 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                    >
                        <Users className="h-4 w-4 text-ui-sub" />
                        管理账户角色
                    </Link>
                }
            />

            {/* 三级内置角色视觉卡片 */}
            <div className="grid gap-4 lg:grid-cols-3">
                {roles.map((role) => {
                    const desc = ROLE_DESCRIPTIONS[role.key];
                    const visual = ROLE_VISUALS[role.key];
                    const Icon = visual.icon;
                    const ratio = Math.round((role.permissions.length / totalPermCount) * 100);

                    return (
                        <section key={role.id} className="studio-card flex flex-col p-5">
                            <div className="flex items-start justify-between gap-3">
                                <div className="flex items-center gap-3">
                                    <span
                                        className={cn(
                                            'grid h-10 w-10 shrink-0 place-items-center rounded-xl',
                                            visual.iconBg,
                                            visual.iconColor,
                                        )}
                                    >
                                        <Icon className="h-5 w-5" />
                                    </span>
                                    <div>
                                        <h2 className="text-[16px] font-semibold tracking-[-0.01em] text-ui-text">
                                            {desc.title}
                                        </h2>
                                        <span className="font-code text-[11.5px] text-ui-sub">
                                            role:{role.key}
                                        </span>
                                    </div>
                                </div>
                                <span className="tnum inline-flex items-center gap-1 rounded-full bg-ui-bg px-2.5 py-1 text-[12px] font-medium text-ui-sub">
                                    {role.user_count} 个账户
                                </span>
                            </div>

                            <p className="mt-3.5 text-[13px] leading-relaxed text-ui-sub">
                                {desc.summary}
                            </p>

                            <ul className="mt-4 flex-1 space-y-2 border-t border-ui-line pt-4 text-[13px] text-ui-text">
                                {visual.highlights.map((item) => (
                                    <li key={item} className="flex items-start gap-2">
                                        <span className="mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-full bg-ok-tint text-ok">
                                            <Check className="h-2.5 w-2.5" strokeWidth={2.75} />
                                        </span>
                                        <span>{item}</span>
                                    </li>
                                ))}
                            </ul>

                            {/* 权限覆盖进度条 */}
                            <div className="mt-5 border-t border-ui-line pt-3.5">
                                <div className="flex items-center justify-between text-[12px]">
                                    <span className="text-ui-sub">权限覆盖范围</span>
                                    <span className="tnum font-medium text-ui-text">
                                        {role.permissions.length} / {allPermissions.length} 项
                                    </span>
                                </div>
                                <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-[#E7EFED]">
                                    <div
                                        className={cn('h-full rounded-full', visual.barColor)}
                                        style={{ width: `${ratio}%` }}
                                    />
                                </div>
                            </div>
                        </section>
                    );
                })}
            </div>

            {/* 分组权限矩阵 */}
            <section aria-labelledby="role-matrix-h" className="studio-card overflow-hidden">
                <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ui-line px-5 py-4">
                    <div>
                        <h2 id="role-matrix-h" className="text-[15.5px] font-semibold text-ui-text">
                            权限明细矩阵
                        </h2>
                        <p className="mt-0.5 text-[12.5px] text-ui-sub">
                            按业务领域分组展示服务端实时返回的权限点。
                        </p>
                    </div>
                    <span className="tnum rounded-full bg-ui-bg px-2.5 py-1 text-[12px] text-ui-sub">
                        共 {allPermissions.length} 项权限
                    </span>
                </div>

                <div className="overflow-x-auto">
                    <table className="tbl min-w-[680px]">
                        <thead>
                            <tr>
                                <th className="w-[26%]">权限标识</th>
                                <th className="w-[38%]">能力说明</th>
                                {roles.map((role) => (
                                    <th key={role.id} className="text-center">
                                        {ROLE_DESCRIPTIONS[role.key].title}
                                    </th>
                                ))}
                            </tr>
                        </thead>
                        <tbody>
                            {groupedPermissions.map(([groupName, perms]) => (
                                <Fragment key={groupName}>
                                    <tr className="hover:bg-transparent">
                                        <td
                                            colSpan={2 + roles.length}
                                            className="bg-[#F5F9F8] py-2 text-[11.5px] font-semibold tracking-[0.04em] text-accent-2 uppercase"
                                        >
                                            {groupName}
                                        </td>
                                    </tr>
                                    {perms.map((perm) => (
                                        <tr key={perm}>
                                            <td className="font-code text-[12.5px] text-ui-text">
                                                {perm}
                                            </td>
                                            <td className="text-[13px] text-ui-sub">
                                                {PERMISSION_LABELS[perm] ?? '—'}
                                            </td>
                                            {roles.map((role) => {
                                                const granted = role.permissions.includes(perm);
                                                return (
                                                    <td key={role.id} className="text-center">
                                                        {granted ? (
                                                            <span
                                                                className="inline-grid h-6 w-6 place-items-center rounded-full bg-ok-tint text-ok"
                                                                aria-label="已授予"
                                                            >
                                                                <Check
                                                                    className="h-3.5 w-3.5"
                                                                    strokeWidth={2.5}
                                                                />
                                                            </span>
                                                        ) : (
                                                            <span
                                                                className="inline-grid h-6 w-6 place-items-center text-[#BCCAC7]"
                                                                aria-label="未授予"
                                                            >
                                                                <Minus className="h-3.5 w-3.5" />
                                                            </span>
                                                        )}
                                                    </td>
                                                );
                                            })}
                                        </tr>
                                    ))}
                                </Fragment>
                            ))}
                        </tbody>
                    </table>
                </div>

                <p className="border-t border-ui-line bg-ui-subtle/70 px-5 py-3 text-[12px] text-ui-sub">
                    前端导航与按钮仅按权限做展示控制，真实授权边界一律由 Rust API 服务端强制校验。
                </p>
            </section>
        </PageContainer>
    );
}
