import type { Schemas } from '@silva/api-client';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import {
    Activity,
    ArrowRight,
    ArrowUpRight,
    CheckCircle2,
    Clock,
    FilePlus2,
    FileText,
    Globe,
    Image as ImageIcon,
    Send,
} from 'lucide-react';
import { PageContainer, PageHeader } from '@/components/ui/feedback';
import { StatusChip, type ChipTone } from '@/components/ui/status-chip';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, fetchHealthLive, unwrapResponse } from '@/lib/api';
import { formatGeneration, formatRelativeTime, formatShortDateTime } from '@/lib/format';
import { getPrimaryRoleLabel, hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function reviewStatusChip(
    revNo: number,
    review: Schemas['ReviewSummary'] | null,
): { label: string; tone: ChipTone } {
    if (!review) return { label: `r${revNo} 草稿`, tone: 'warn' };
    if (review.status === 'pending') return { label: `r${revNo} 审核中`, tone: 'info' };
    if (review.status === 'approved') return { label: `r${revNo} 已批准`, tone: 'ok' };
    return { label: `r${revNo} 已拒绝`, tone: 'bad' };
}

export function publishJobStatusMeta(status: Schemas['PublishJobStatus']): {
    label: string;
    tone: ChipTone;
} {
    switch (status) {
        case 'queued':
            return { label: '排队中', tone: 'warn' };
        case 'building':
            return { label: '构建中', tone: 'info' };
        case 'active':
            return { label: '已上线', tone: 'ok' };
        case 'failed':
            return { label: '构建失败', tone: 'bad' };
        case 'superseded':
            return { label: '已被更新的目标替代', tone: 'mute' };
        case 'retired':
            return { label: '已被新版替换', tone: 'mute' };
    }
}

export function DashboardPage() {
    const { user } = useAuth();
    const roleLabel = getPrimaryRoleLabel(user?.roles);
    const canCreatePost = hasPermission(user, PERMISSIONS.POSTS_CREATE);
    const canReview = hasPermission(user, PERMISSIONS.POSTS_REVIEW);
    const canManageMedia =
        hasPermission(user, PERMISSIONS.MEDIA_MANAGE_OWN) ||
        hasPermission(user, PERMISSIONS.MEDIA_MANAGE);
    const canReadJobs =
        hasPermission(user, PERMISSIONS.JOBS_READ) ||
        hasPermission(user, PERMISSIONS.JOBS_READ_OWN);
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

    const pendingReviewsQuery = useQuery({
        queryKey: queryKeys.posts.list({ review_status: 'pending', page: 1, per_page: 5 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: { query: { review_status: 'pending', page: 1, per_page: 5 } },
                }),
            ),
        enabled: canReview,
    });

    const rejectedMineQuery = useQuery({
        queryKey: queryKeys.posts.list({
            author_id: user?.id,
            review_status: 'rejected',
            page: 1,
            per_page: 1,
        }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: {
                        query: {
                            author_id: user?.id,
                            review_status: 'rejected',
                            page: 1,
                            per_page: 1,
                        },
                    },
                }),
            ),
        enabled: isAuthorOnly && Boolean(user?.id),
    });

    const pendingMineQuery = useQuery({
        queryKey: queryKeys.posts.list({
            author_id: user?.id,
            review_status: 'pending',
            page: 1,
            per_page: 1,
        }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: {
                        query: {
                            author_id: user?.id,
                            review_status: 'pending',
                            page: 1,
                            per_page: 1,
                        },
                    },
                }),
            ),
        enabled: isAuthorOnly && Boolean(user?.id),
    });

    const publishJobsQuery = useQuery({
        queryKey: queryKeys.publishing.jobs({ page: 1, per_page: 4 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/publishing/jobs', {
                    params: { query: { page: 1, per_page: 4 } },
                }),
            ),
        enabled: canReadJobs,
    });

    const publicSiteQuery = useQuery({
        queryKey: queryKeys.publicSite.current,
        queryFn: async () => {
            const res = await unwrapResponse(apiClient.GET('/api/v1/public/site'));
            return res.data;
        },
        retry: false,
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
    const pendingPosts = pendingReviewsQuery.data?.data ?? [];
    const pendingTotal = pendingReviewsQuery.data?.meta.pagination.total ?? 0;
    const jobs = publishJobsQuery.data?.data ?? [];
    const latestJob = jobs[0];

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="WORKSPACE"
                title={`你好，${user?.display_name ?? ''}`}
                description={`当前角色为 ${roleLabel}。这里汇集了你有权处理的内容、审核与站点运行状态。`}
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
                        {canManageMedia ? (
                            <Link
                                to="/media"
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                <ImageIcon className="h-4 w-4 text-ui-sub" />
                                媒体库
                            </Link>
                        ) : null}
                        {canReview ? (
                            <Link
                                to="/posts"
                                search={{ review_status: 'pending' }}
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                <CheckCircle2 className="h-4 w-4 text-accent" />
                                处理待审核
                                {pendingTotal > 0 ? (
                                    <span className="tnum rounded-full bg-warn-tint px-1.5 py-0.5 text-[11px] font-semibold text-warn">
                                        {pendingTotal}
                                    </span>
                                ) : null}
                            </Link>
                        ) : null}
                    </>
                }
            />

            {/* KPI 概览卡片带（按角色隔离 · 参考图双层卡片结构 + 首卡翡翠绿主视觉） */}
            {isAuthorOnly ? (
                <div className="grid gap-4 sm:grid-cols-3">
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

                    <Link
                        to="/posts"
                        search={{ review_status: 'pending' }}
                        className="studio-card-interactive group flex flex-col overflow-hidden"
                    >
                        <div className="flex-1 p-5">
                            <div className="flex items-center gap-3">
                                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-warn-tint text-warn">
                                    <Clock className="h-5 w-5" />
                                </span>
                                <div className="min-w-0">
                                    <div className="text-[14px] font-semibold text-ui-text">
                                        审核中
                                    </div>
                                    <div className="truncate text-[12px] text-ui-sub">
                                        等待 Editor 审核确认
                                    </div>
                                </div>
                            </div>
                            <div className="tnum mt-5 text-[30px] leading-none font-semibold tracking-[-0.025em] text-ui-text">
                                {pendingMineQuery.data?.meta.pagination.total ?? 0}
                            </div>
                        </div>
                        <div className="flex items-center justify-between border-t border-[#EFF3F1] px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>查看送审状态</span>
                            <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                        </div>
                    </Link>

                    <Link
                        to="/posts"
                        search={{ review_status: 'rejected' }}
                        className="studio-card-interactive group flex flex-col overflow-hidden"
                    >
                        <div className="flex-1 p-5">
                            <div className="flex items-center gap-3">
                                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-bad-tint text-bad">
                                    <FileText className="h-5 w-5" />
                                </span>
                                <div className="min-w-0">
                                    <div className="text-[14px] font-semibold text-ui-text">
                                        待修改
                                    </div>
                                    <div className="truncate text-[12px] text-ui-sub">
                                        查看拒绝意见并修订
                                    </div>
                                </div>
                            </div>
                            <div className="tnum mt-5 text-[30px] leading-none font-semibold tracking-[-0.025em] text-bad">
                                {rejectedMineQuery.data?.meta.pagination.total ?? 0}
                            </div>
                        </div>
                        <div className="flex items-center justify-between border-t border-[#EFF3F1] px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>立即处理修订</span>
                            <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                        </div>
                    </Link>
                </div>
            ) : (
                <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
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
                                    <div className="text-[14px] font-semibold">文章总数</div>
                                    <div className="truncate text-[12px] text-white/75">
                                        我的文章 {myTotal} 篇
                                    </div>
                                </div>
                            </div>
                            <div className="mt-5 flex items-baseline gap-2.5">
                                <span className="tnum text-[30px] leading-none font-semibold tracking-[-0.025em]">
                                    {allPostsTotal}
                                </span>
                                <span className="rounded-full bg-white/18 px-2 py-0.5 text-[11px] font-medium text-white">
                                    全站内容
                                </span>
                            </div>
                        </div>
                        <div className="flex items-center justify-between bg-white px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>管理文章</span>
                            <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                        </div>
                    </Link>

                    <Link
                        to="/posts"
                        search={{ review_status: 'pending' }}
                        className="studio-card-interactive group flex flex-col overflow-hidden"
                    >
                        <div className="flex-1 p-5">
                            <div className="flex items-center gap-3">
                                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-warn-tint text-warn">
                                    <Clock className="h-5 w-5" />
                                </span>
                                <div className="min-w-0">
                                    <div className="text-[14px] font-semibold text-ui-text">
                                        待审核修订
                                    </div>
                                    <div className="truncate text-[12px] text-ui-sub">
                                        {pendingTotal > 0 ? '有待处理送审' : '当前无积压审核'}
                                    </div>
                                </div>
                            </div>
                            <div className="tnum mt-5 text-[30px] leading-none font-semibold tracking-[-0.025em] text-ui-text">
                                {pendingTotal}
                            </div>
                        </div>
                        <div className="flex items-center justify-between border-t border-[#EFF3F1] px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>进入审核队列</span>
                            <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                        </div>
                    </Link>

                    <Link
                        to="/publishing"
                        className="studio-card-interactive group flex flex-col overflow-hidden"
                    >
                        <div className="flex-1 p-5">
                            <div className="flex items-center gap-3">
                                <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-accent-tint text-accent">
                                    <Globe className="h-5 w-5" />
                                </span>
                                <div className="min-w-0">
                                    <div className="text-[14px] font-semibold text-ui-text">
                                        线上站点快照
                                    </div>
                                    <div className="truncate text-[12px] text-ui-sub">
                                        {latestJob
                                            ? `最新 ${formatGeneration(latestJob.generation)} · ${publishJobStatusMeta(latestJob.status).label}`
                                            : '尚无发布任务'}
                                    </div>
                                </div>
                            </div>
                            <div className="tnum mt-5 font-code text-[24px] leading-none font-semibold tracking-[-0.02em] text-ui-text">
                                {publicSiteQuery.data
                                    ? formatGeneration(publicSiteQuery.data.generation)
                                    : '未上线'}
                            </div>
                        </div>
                        <div className="flex items-center justify-between border-t border-[#EFF3F1] px-5 py-3 text-[12.5px] font-medium text-ui-sub transition-colors group-hover:text-accent">
                            <span>查看发布记录</span>
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
            <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
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
                                创建第一篇 Markdown 文章，体验不可变修订与安全预览流程。
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
                                        <th>编辑状态</th>
                                        <th>当前线上</th>
                                        <th className="text-right">最近更新</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {myPosts.map((post) => {
                                        const editMeta = reviewStatusChip(
                                            post.current_revision_no,
                                            post.current_review,
                                        );
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
                                                    <StatusChip tone={editMeta.tone}>
                                                        {editMeta.label}
                                                    </StatusChip>
                                                </td>
                                                <td>
                                                    {post.live_revision_id ? (
                                                        <StatusChip tone="ok">已上线</StatusChip>
                                                    ) : (
                                                        <StatusChip tone="mute">未上线</StatusChip>
                                                    )}
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

                {/* 右侧边栏卡片堆栈 */}
                <div className="space-y-5">
                    {canReview ? (
                        <section
                            aria-labelledby="dash-review-q"
                            className="studio-card overflow-hidden"
                        >
                            <div className="flex h-13 items-center justify-between border-b border-ui-line px-5">
                                <h2
                                    id="dash-review-q"
                                    className="flex items-center gap-2 text-[15px] font-semibold text-ui-text"
                                >
                                    待审核修订
                                    <span className="tnum rounded-full bg-ui-bg px-2 py-0.5 text-[11.5px] font-medium text-ui-sub">
                                        {pendingTotal}
                                    </span>
                                </h2>
                                <Link
                                    to="/posts"
                                    search={{ review_status: 'pending' }}
                                    className="text-[13px] font-medium text-accent hover:underline"
                                >
                                    去审核
                                </Link>
                            </div>
                            {pendingReviewsQuery.isLoading ? (
                                <div className="p-5">
                                    <div className="h-12 animate-pulse rounded-lg bg-ui-bg" />
                                </div>
                            ) : pendingPosts.length === 0 ? (
                                <div className="px-5 py-9 text-center text-[13.5px] text-ui-sub">
                                    当前没有待审核的修订，队列已清空。
                                </div>
                            ) : (
                                <ul className="divide-y divide-ui-line text-[13.5px]">
                                    {pendingPosts.map((post) => (
                                        <li
                                            key={post.id}
                                            className="px-5 py-3.5 transition-colors hover:bg-ui-subtle"
                                        >
                                            <Link
                                                to="/posts/$postId/revisions/$revisionId"
                                                params={{
                                                    postId: post.id,
                                                    revisionId: post.current_revision_id,
                                                }}
                                                className="font-medium text-ui-text hover:text-accent"
                                            >
                                                {post.title}
                                            </Link>
                                            <div className="mt-1 flex items-center justify-between text-[12px] text-ui-sub">
                                                <span>
                                                    r{post.current_revision_no} ·{' '}
                                                    {post.author?.display_name ?? '未知作者'}
                                                </span>
                                                <span className="tnum">
                                                    {formatRelativeTime(post.updated_at)}
                                                </span>
                                            </div>
                                        </li>
                                    ))}
                                </ul>
                            )}
                        </section>
                    ) : null}

                    {canReadJobs ? (
                        <section aria-labelledby="dash-pub" className="studio-card overflow-hidden">
                            <div className="flex h-13 items-center justify-between border-b border-ui-line px-5">
                                <h2
                                    id="dash-pub"
                                    className="flex items-center gap-2 text-[15px] font-semibold text-ui-text"
                                >
                                    <Send className="h-4 w-4 text-accent" />
                                    近期站点发布
                                </h2>
                                <Link
                                    to="/publishing"
                                    className="text-[13px] font-medium text-accent hover:underline"
                                >
                                    全部任务
                                </Link>
                            </div>
                            {jobs.length === 0 ? (
                                <p className="px-5 py-8 text-center text-[13.5px] text-ui-sub">
                                    暂无发布任务记录。
                                </p>
                            ) : (
                                <ul className="divide-y divide-ui-line text-[13.5px]">
                                    {jobs.map((job) => {
                                        const meta = publishJobStatusMeta(job.status);
                                        return (
                                            <li
                                                key={job.id}
                                                className="flex items-center justify-between gap-3 px-5 py-3 transition-colors hover:bg-ui-subtle"
                                            >
                                                <Link
                                                    to="/publishing/$jobId"
                                                    params={{ jobId: job.id }}
                                                    className="min-w-0 flex-1"
                                                >
                                                    <div className="flex items-center gap-2">
                                                        <span className="tnum font-code text-[13px] font-semibold text-ui-text hover:text-accent">
                                                            {formatGeneration(job.generation)}
                                                        </span>
                                                        <span className="truncate font-code text-[12px] text-ui-sub">
                                                            {job.trigger_type}
                                                        </span>
                                                    </div>
                                                    <div className="tnum mt-0.5 text-[11.5px] text-ui-sub">
                                                        {formatShortDateTime(job.created_at)}
                                                    </div>
                                                </Link>
                                                <StatusChip tone={meta.tone}>
                                                    {meta.label}
                                                </StatusChip>
                                            </li>
                                        );
                                    })}
                                </ul>
                            )}
                        </section>
                    ) : null}
                </div>
            </div>
        </PageContainer>
    );
}
