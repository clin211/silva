import { postPublicationStatus } from '@/features/posts/post-publication-status';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import {
    Activity,
    ArrowRight,
    ArrowUpRight,
    BookOpen,
    FilePlus2,
    FileText,
    Image as ImageIcon,
} from 'lucide-react';
import { PageContainer, PageHeader } from '@/components/ui/feedback';
import { StatusChip } from '@/components/ui/status-chip';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, fetchHealthLive, unwrapResponse } from '@/lib/api';
import { formatRelativeTime } from '@/lib/format';
import { getPrimaryRoleLabel, hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function DashboardPage() {
    const { user } = useAuth();
    const roleLabel = getPrimaryRoleLabel(user?.roles);
    const canCreatePost = hasPermission(user, PERMISSIONS.POSTS_CREATE);
    const canManageSeries = hasPermission(user, PERMISSIONS.SERIES_MANAGE);
    const canManageMedia =
        hasPermission(user, PERMISSIONS.MEDIA_MANAGE_OWN) ||
        hasPermission(user, PERMISSIONS.MEDIA_MANAGE);
    const isAdmin =
        hasPermission(user, PERMISSIONS.AUDIT_READ) ||
        hasPermission(user, PERMISSIONS.USERS_MANAGE);
    const isAuthorOnly = !hasPermission(user, PERMISSIONS.POSTS_MANAGE);

    const myPostsQuery = useQuery({
        queryKey: queryKeys.posts.list({ author_id: user?.id, page: 1, per_page: 6 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: { query: { author_id: user?.id, page: 1, per_page: 6 } },
                }),
            ),
        enabled: Boolean(user?.id),
    });

    const allPostsCountQuery = useQuery({
        queryKey: queryKeys.posts.list({ page: 1, per_page: 1 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: { query: { page: 1, per_page: 1 } },
                }),
            ),
        enabled: !isAuthorOnly,
    });

    const allSeriesCountQuery = useQuery({
        queryKey: queryKeys.series.list({ page: 1, per_page: 1 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/series', {
                    params: { query: { page: 1, per_page: 1 } },
                }),
            ),
        enabled: canManageSeries,
    });

    const healthQuery = useQuery({
        queryKey: queryKeys.system.health,
        queryFn: fetchHealthLive,
        enabled: isAdmin,
        retry: false,
    });

    const myPosts = myPostsQuery.data?.data ?? [];
    const myTotal = myPostsQuery.data?.meta.pagination.total ?? 0;
    const allPostsTotal = allPostsCountQuery.data?.meta.pagination.total ?? 0;
    const allSeriesTotal = allSeriesCountQuery.data?.meta.pagination.total ?? 0;

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="WORKSPACE"
                title={`你好，${user?.display_name ?? ''}`}
                description={`当前角色为 ${roleLabel}。这里汇集了你有权处理的内容与站点运行状态。`}
                actions={
                    <>
                        {canCreatePost ? (
                            <Link
                                to="/posts/new"
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] bg-accent px-4 text-[13.5px] font-medium text-white shadow-[0_1px_2px_rgba(8,127,137,0.28)] transition-colors hover:bg-accent-2"
                            >
                                <FilePlus2 className="h-4 w-4" />
                                新建文章
                            </Link>
                        ) : null}
                        {canManageSeries ? (
                            <Link
                                to="/series"
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                <BookOpen className="h-4 w-4 text-ui-sub" />
                                博客管理
                            </Link>
                        ) : null}
                        {canManageMedia ? (
                            <Link
                                to="/media"
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                <ImageIcon className="h-4 w-4 text-ui-sub" />
                                媒体库
                            </Link>
                        ) : null}
                    </>
                }
            />

            {/* KPI 概览卡片带（按角色隔离 · 参考图双层卡片结构 + 首卡翡翠绿主视觉） */}
            {isAuthorOnly ? (
                <div className="grid gap-4">
                    <Link
                        to="/posts"
                        className="studio-card-interactive group flex flex-col overflow-hidden"
                    >
                        <div className="hero-emerald-card flex-1 p-5 text-white">
                            <div className="flex items-center gap-3">
                                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-white/15 text-white backdrop-blur-xs">
                                    <FileText className="h-5 w-5" />
                                </span>
                                <div className="min-w-0">
                                    <div className="text-[14px] font-semibold">我的文章</div>
                                    <div className="truncate text-[12px] text-white/75">
                                        {myPosts[0]
                                            ? `最近更新于 ${formatRelativeTime(myPosts[0].updated_at)}`
                                            : '开始撰写第一篇文章'}
                                    </div>
                                </div>
                            </div>
                            <div className="tnum mt-5 text-[30px] leading-none font-semibold tracking-[-0.025em]">
                                {myTotal}
                            </div>
                        </div>
                        <div className="flex items-center justify-between bg-white px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>查看文章列表</span>
                            <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                        </div>
                    </Link>
                </div>
            ) : (
                <div className="grid gap-4 sm:grid-cols-2 ">
                    <Link
                        to={canManageSeries ? '/series' : '/posts'}
                        className="studio-card-interactive group flex flex-col overflow-hidden"
                    >
                        <div className="hero-emerald-card flex-1 p-5 text-white">
                            <div className="flex items-center gap-3">
                                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-white/15 text-white backdrop-blur-xs">
                                    {canManageSeries ? (
                                        <BookOpen className="h-5 w-5" />
                                    ) : (
                                        <FileText className="h-5 w-5" />
                                    )}
                                </span>
                                <div className="min-w-0">
                                    <div className="text-[14px] font-semibold">
                                        {canManageSeries ? '专栏总数' : '文章总数'}
                                    </div>
                                    <div className="truncate text-[12px] text-white/75">
                                        {canManageSeries
                                            ? `全站共 ${allPostsTotal} 篇文章`
                                            : `我的文章 ${myTotal} 篇`}
                                    </div>
                                </div>
                            </div>
                            <div className="mt-5 flex items-baseline gap-2.5">
                                <span className="tnum text-[30px] leading-none font-semibold tracking-[-0.025em]">
                                    {canManageSeries ? allSeriesTotal : allPostsTotal}
                                </span>
                                <span className="rounded-full bg-white/18 px-2 py-0.5 text-[11px] font-medium text-white">
                                    {canManageSeries ? '专栏体系' : '全站内容'}
                                </span>
                            </div>
                        </div>
                        <div className="flex items-center justify-between bg-white px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>{canManageSeries ? '管理专栏' : '管理文章'}</span>
                            <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                        </div>
                    </Link>

                    {isAdmin ? (
                        <Link
                            to="/system/health"
                            className="studio-card-interactive group flex flex-col overflow-hidden"
                        >
                            <div className="flex-1 p-5">
                                <div className="flex items-center gap-3">
                                    <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-ok-tint text-ok">
                                        <Activity className="h-5 w-5" />
                                    </span>
                                    <div className="min-w-0">
                                        <div className="text-[14px] font-semibold text-ui-text">
                                            API 服务探针
                                        </div>
                                        <div className="truncate text-[12px] text-ui-sub">
                                            {healthQuery.isError
                                                ? '/health/live 不可达'
                                                : '存活探针正常'}
                                        </div>
                                    </div>
                                </div>
                                <div className="tnum mt-5 text-[24px] leading-none font-semibold tracking-[-0.02em] text-ui-text">
                                    {healthQuery.isLoading
                                        ? '…'
                                        : healthQuery.isError
                                          ? '异常'
                                          : `${healthQuery.data?.latencyMs} ms`}
                                </div>
                            </div>
                            <div className="flex items-center justify-between border-t border-[#EFF3F1] px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                                <span>查看运行指标</span>
                                <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                            </div>
                        </Link>
                    ) : (
                        <Link
                            to="/media"
                            className="studio-card-interactive group flex flex-col overflow-hidden"
                        >
                            <div className="flex-1 p-5">
                                <div className="flex items-center gap-3">
                                    <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent-tint text-accent">
                                        <ImageIcon className="h-5 w-5" />
                                    </span>
                                    <div className="min-w-0">
                                        <div className="text-[14px] font-semibold text-ui-text">
                                            媒体资源库
                                        </div>
                                        <div className="truncate text-[12px] text-ui-sub">
                                            流式上传与封面管理
                                        </div>
                                    </div>
                                </div>
                                <div className="mt-5 text-[22px] leading-none font-semibold text-ui-text">
                                    管理图片
                                </div>
                            </div>
                            <div className="flex items-center justify-between border-t border-[#EFF3F1] px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                                <span>打开媒体库</span>
                                <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                            </div>
                        </Link>
                    )}
                </div>
            )}

            {/* 主体 Bento 工作区 */}
            <div className="grid items-start gap-5 ">
                {/* 左侧：我最近编辑的文章 */}
                <section
                    aria-labelledby="dash-my-posts"
                    className="studio-card min-w-0 overflow-hidden"
                >
                    <div className="flex h-13 items-center justify-between border-b border-ui-line px-5">
                        <div className="flex items-center gap-2">
                            <h2
                                id="dash-my-posts"
                                className="text-[15px] font-semibold text-ui-text"
                            >
                                我最近编辑的文章
                            </h2>
                            <span className="tnum rounded-full bg-ui-bg px-2 py-0.5 text-[11.5px] font-medium text-ui-sub">
                                {myTotal}
                            </span>
                        </div>
                        <Link
                            to="/posts"
                            className="inline-flex items-center gap-1 text-[13px] font-medium text-accent hover:underline"
                        >
                            全部文章
                            <ArrowUpRight className="h-3.5 w-3.5" />
                        </Link>
                    </div>
                    {myPostsQuery.isLoading ? (
                        <div className="space-y-2.5 p-5">
                            <div className="h-10 animate-pulse rounded-lg bg-ui-bg" />
                            <div className="h-10 animate-pulse rounded-lg bg-ui-bg" />
                            <div className="h-10 animate-pulse rounded-lg bg-ui-bg" />
                        </div>
                    ) : myPosts.length === 0 ? (
                        <div className="flex flex-col items-center justify-center px-6 py-14 text-center">
                            <div className="grid h-11 w-11 place-items-center rounded-2xl bg-accent-tint text-accent">
                                <FileText className="h-5 w-5" />
                            </div>
                            <p className="mt-3 text-[14.5px] font-medium text-ui-text">
                                你还没有写过文章
                            </p>
                            <p className="mt-1 text-[13px] text-ui-sub">
                                创建第一篇 Markdown 文章。
                            </p>
                            {canCreatePost ? (
                                <Link
                                    to="/posts/new"
                                    className="mt-4 inline-flex h-9 items-center gap-1.5 rounded-[9px] bg-accent px-4 text-[13.5px] font-medium text-white hover:bg-accent-2"
                                >
                                    <FilePlus2 className="h-4 w-4" />
                                    写第一篇文章
                                </Link>
                            ) : null}
                        </div>
                    ) : (
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[540px]">
                                <thead>
                                    <tr>
                                        <th>标题</th>
                                        <th>发布状态</th>
                                        <th className="text-right">最近更新</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {myPosts.map((post) => {
                                        const publication = postPublicationStatus(post);
                                        return (
                                            <tr key={post.id}>
                                                <td>
                                                    <Link
                                                        to="/posts/$postId/edit"
                                                        params={{ postId: post.id }}
                                                        className="font-medium text-ui-text transition-colors hover:text-accent"
                                                    >
                                                        {post.title}
                                                    </Link>
                                                    <div className="mt-0.5 font-code text-[11.5px] text-ui-sub">
                                                        /{post.slug}
                                                    </div>
                                                </td>
                                                <td>
                                                    <StatusChip tone={publication.tone}>
                                                        {publication.label}
                                                    </StatusChip>
                                                </td>
                                                <td className="tnum text-right text-[12.5px] text-ui-sub">
                                                    {formatRelativeTime(post.updated_at)}
                                                </td>
                                            </tr>
                                        );
                                    })}
                                </tbody>
                            </table>
                        </div>
                    )}
                </section>
            </div>
        </PageContainer>
    );
}
