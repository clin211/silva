import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';
import {
    ErrorBanner,
    LoadingCard,
    PageContainer,
    PageHeader,
    PaginationBar,
} from '@/components/ui/feedback';
import { StatusChip } from '@/components/ui/status-chip';
import { useAuth } from '@/features/auth/auth-context';
import { reviewStatusChip } from '@/features/dashboard/dashboard-page';
import { apiClient, unwrapResponse } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatShortDateTime } from '@/lib/format';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function PostRevisionsPage({ postId }: { postId: string }) {
    const { user } = useAuth();
    const canReview = hasPermission(user, PERMISSIONS.POSTS_REVIEW);
    const [page, setPage] = useState(1);

    const postQuery = useQuery({
        queryKey: queryKeys.posts.detail(postId),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}', {
                    params: { path: { id: postId } },
                }),
            );
            return res.data;
        },
    });

    const revisionsQuery = useQuery({
        queryKey: queryKeys.posts.revisions(postId, page, 20),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}/revisions', {
                    params: { path: { id: postId }, query: { page, per_page: 20 } },
                }),
            ),
    });

    if (postQuery.isLoading || revisionsQuery.isLoading) {
        return <LoadingCard text="正在加载修订历史…" />;
    }

    if (postQuery.isError) {
        return (
            <ErrorBanner
                title="文章读取失败"
                error={postQuery.error}
                onRetry={() => void postQuery.refetch()}
            />
        );
    }

    const post = postQuery.data!;
    const revisions = revisionsQuery.data?.data ?? [];
    const pagination = revisionsQuery.data?.meta.pagination;

    const currentChip = reviewStatusChip(
        post.current_revision.revision_no,
        post.current_revision.review,
    );

    return (
        <PageContainer className="space-y-5">
            <PageHeader
                eyebrow="REVISIONS"
                title="修订历史"
                description={post.current_revision.title}
                actions={
                    <Link
                        to="/posts/$postId/edit"
                        params={{ postId }}
                        className="inline-flex h-9 items-center rounded-[9px] border border-ui-line bg-white px-4 text-[13.5px] font-medium text-ui-text shadow-2xs hover:bg-ui-subtle"
                    >
                        返回编辑
                    </Link>
                }
            />

            {/* 三种修订概览 */}
            <dl className="grid gap-4 sm:grid-cols-3">
                <div className="studio-card px-5 py-4">
                    <dt className="text-[12.5px] text-ui-sub">当前编辑修订</dt>
                    <dd className="mt-2">
                        <StatusChip tone={currentChip.tone}>{currentChip.label}</StatusChip>
                    </dd>
                </div>
                <div className="studio-card px-5 py-4">
                    <dt className="text-[12.5px] text-ui-sub">期望公开修订</dt>
                    <dd className="mt-2">
                        {post.publication.desired_published_revision_id ? (
                            <StatusChip tone="info">已设置目标</StatusChip>
                        ) : post.publication.live_revision_id ? (
                            <StatusChip tone="warn">取消发布 · 待上线</StatusChip>
                        ) : (
                            <StatusChip tone="mute">无</StatusChip>
                        )}
                    </dd>
                </div>
                <div className="studio-card px-5 py-4">
                    <dt className="text-[12.5px] text-ui-sub">当前线上修订</dt>
                    <dd className="mt-2">
                        {post.publication.live_revision_id ? (
                            <StatusChip tone="ok">已上线</StatusChip>
                        ) : (
                            <StatusChip tone="mute">未上线</StatusChip>
                        )}
                    </dd>
                </div>
            </dl>

            {revisionsQuery.isError ? (
                <ErrorBanner
                    title="修订历史加载失败"
                    error={revisionsQuery.error}
                    onRetry={() => void revisionsQuery.refetch()}
                />
            ) : (
                <div className="studio-card overflow-hidden">
                    <div className="overflow-x-auto">
                        <table className="tbl min-w-[740px]">
                            <thead>
                                <tr>
                                    <th>修订</th>
                                    <th>标题</th>
                                    <th>保存时间</th>
                                    <th>作者</th>
                                    <th>审核状态</th>
                                    <th className="text-right">操作</th>
                                </tr>
                            </thead>
                            <tbody>
                                {revisions.map((rev) => {
                                    const isCurrent = rev.id === post.current_revision_id;
                                    const isDesired =
                                        rev.id === post.publication.desired_published_revision_id;
                                    const isLive = rev.id === post.publication.live_revision_id;
                                    const reviewChip = reviewStatusChip(
                                        rev.revision_no,
                                        rev.review,
                                    );

                                    return (
                                        <tr
                                            key={rev.id}
                                            className={cn(isDesired && 'bg-accent-tint/35')}
                                        >
                                            <td className="whitespace-nowrap">
                                                <Link
                                                    to="/posts/$postId/revisions/$revisionId"
                                                    params={{ postId, revisionId: rev.id }}
                                                    className="tnum font-medium text-ui-text hover:text-accent"
                                                >
                                                    r{rev.revision_no}
                                                </Link>
                                                {isCurrent ? (
                                                    <span className="ml-2 text-[12px] text-ui-sub">
                                                        当前编辑
                                                    </span>
                                                ) : null}
                                                {isDesired ? (
                                                    <span className="ml-2 text-[12px] text-accent">
                                                        期望公开
                                                    </span>
                                                ) : null}
                                                {isLive ? (
                                                    <span className="ml-2 text-[12px] text-ok">
                                                        当前线上
                                                    </span>
                                                ) : null}
                                            </td>
                                            <td className="max-w-[280px] truncate">{rev.title}</td>
                                            <td className="tnum whitespace-nowrap text-ui-sub">
                                                {formatShortDateTime(rev.created_at)}
                                            </td>
                                            <td>{rev.created_by?.display_name ?? '—'}</td>
                                            <td>
                                                <StatusChip tone={reviewChip.tone}>
                                                    {rev.review
                                                        ? rev.review.status === 'pending'
                                                            ? '审核中'
                                                            : rev.review.status === 'approved'
                                                              ? '已批准'
                                                              : '已拒绝'
                                                        : '未提交'}
                                                </StatusChip>
                                            </td>
                                            <td className="text-right">
                                                <Link
                                                    to="/posts/$postId/revisions/$revisionId"
                                                    params={{ postId, revisionId: rev.id }}
                                                    className="text-accent hover:underline"
                                                >
                                                    {canReview && rev.review?.status === 'pending'
                                                        ? '审核'
                                                        : '查看'}
                                                </Link>
                                            </td>
                                        </tr>
                                    );
                                })}
                            </tbody>
                        </table>
                    </div>

                    {pagination ? (
                        <PaginationBar
                            page={pagination.page}
                            perPage={pagination.per_page}
                            total={pagination.total}
                            totalPages={pagination.total_pages}
                            unit="个修订"
                            onPageChange={setPage}
                        />
                    ) : null}
                </div>
            )}
        </PageContainer>
    );
}
