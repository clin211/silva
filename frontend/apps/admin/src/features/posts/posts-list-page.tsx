import type { Schemas } from '@silva/api-client';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import { FilePlus2, FileText, Search, X } from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Select } from '@/components/ui/select';
import {
    EmptyState,
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

export interface PostsSearchParams {
    page?: number;
    scope?: 'mine' | 'all';
    q?: string;
    review_status?: Schemas['ReviewStatus'];
    published?: boolean;
    category_id?: string;
    tag_id?: string;
}

export function PostsListPage() {
    const { user } = useAuth();
    const navigate = useNavigate({ from: '/posts' });
    const search = useSearch({ from: '/_authed/posts' });

    const canManageAll = hasPermission(user, PERMISSIONS.POSTS_MANAGE);
    const canCreate = hasPermission(user, PERMISSIONS.POSTS_CREATE);
    const canReview = hasPermission(user, PERMISSIONS.POSTS_REVIEW);

    const effectiveScope: 'mine' | 'all' = !canManageAll ? 'mine' : (search.scope ?? 'all');
    const page = search.page ?? 1;
    const [keywordDraft, setKeywordDraft] = useState<{ base: string; value: string }>({
        base: search.q ?? '',
        value: search.q ?? '',
    });
    const keywordInput =
        keywordDraft.base === (search.q ?? '') ? keywordDraft.value : (search.q ?? '');
    const setKeywordInput = (value: string) => setKeywordDraft({ base: search.q ?? '', value });

    const updateSearch = (patch: Partial<PostsSearchParams>) => {
        void navigate({
            search: (prev) => ({
                ...prev,
                ...patch,
                page: patch.page ?? 1,
            }),
        });
    };

    const categoriesQuery = useQuery({
        queryKey: queryKeys.taxonomy.categories,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/categories')),
    });

    const tagsQuery = useQuery({
        queryKey: queryKeys.taxonomy.tags,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/tags')),
    });

    const queryParams = {
        page,
        per_page: 20,
        q: search.q || undefined,
        author_id: effectiveScope === 'mine' ? user?.id : undefined,
        category_id: search.category_id || undefined,
        tag_id: search.tag_id || undefined,
        review_status: search.review_status || undefined,
        published: search.published,
    };

    const postsQuery = useQuery({
        queryKey: queryKeys.posts.list(queryParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: { query: queryParams },
                }),
            ),
    });

    const posts = postsQuery.data?.data ?? [];
    const pagination = postsQuery.data?.meta.pagination;
    const hasFilters = Boolean(
        search.q ||
        search.review_status ||
        search.published !== undefined ||
        search.category_id ||
        search.tag_id ||
        (canManageAll && effectiveScope === 'mine'),
    );

    const renderDesiredColumn = (post: Schemas['PostSummary']) => {
        if (!post.desired_published_revision_id) {
            if (post.live_revision_id) {
                return <StatusChip tone="warn">取消发布 · 待上线</StatusChip>;
            }
            return <StatusChip tone="mute">未设置</StatusChip>;
        }
        if (post.desired_published_revision_id === post.live_revision_id) {
            return <span className="text-[12.5px] text-ui-sub">与线上一致</span>;
        }
        return <StatusChip tone="info">待上线</StatusChip>;
    };

    return (
        <PageContainer className="space-y-5">
            <PageHeader
                eyebrow="CONTENT"
                title="文章管理"
                description="每篇文章独立追踪「编辑状态」「期望公开」与「当前线上」三条版本线，确保草稿与线上互不干扰。"
                actions={
                    canCreate ? (
                        <Link
                            to="/posts/new"
                            className="inline-flex h-9 items-center gap-1.5 rounded-[9px] bg-accent px-4 text-[13.5px] font-medium text-white shadow-[0_1px_2px_rgba(8,127,137,0.28)] transition-colors hover:bg-accent-2"
                        >
                            <FilePlus2 className="h-4 w-4" />
                            新建文章
                        </Link>
                    ) : null
                }
            />

            {/* 一体化表格与筛选工作卡片 */}
            <div className="studio-card overflow-hidden">
                <search
                    aria-label="筛选文章"
                    className="border-b border-ui-line bg-ui-subtle/70 p-3.5"
                >
                    <form
                        onSubmit={(event) => {
                            event.preventDefault();
                            updateSearch({ q: keywordInput.trim() || undefined });
                        }}
                        className="flex flex-wrap items-center gap-2.5"
                    >
                        {canManageAll ? (
                            <fieldset
                                aria-label="范围"
                                className="inline-flex rounded-[9px] bg-[#E6EEEC] p-0.5 text-[12.5px]"
                            >
                                <button
                                    type="button"
                                    aria-pressed={effectiveScope === 'all'}
                                    onClick={() => updateSearch({ scope: 'all' })}
                                    className="h-7 rounded-[7px] px-3 font-medium text-ui-sub transition-all aria-pressed:bg-white aria-pressed:text-ui-text aria-pressed:shadow-xs"
                                >
                                    全部文章
                                </button>
                                <button
                                    type="button"
                                    aria-pressed={effectiveScope === 'mine'}
                                    onClick={() => updateSearch({ scope: 'mine' })}
                                    className="h-7 rounded-[7px] px-3 font-medium text-ui-sub transition-all aria-pressed:bg-white aria-pressed:text-ui-text aria-pressed:shadow-xs"
                                >
                                    我的文章
                                </button>
                            </fieldset>
                        ) : (
                            <span className="rounded-lg bg-accent-tint px-2.5 py-1 text-[12.5px] font-medium text-accent-2">
                                仅显示我的文章
                            </span>
                        )}

                        <div className="relative min-w-[200px] flex-1 sm:max-w-[260px]">
                            <Search
                                aria-hidden="true"
                                className="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[#889B98]"
                            />
                            <label className="sr-only" htmlFor="post-search-q">
                                搜索标题或 Slug
                            </label>
                            <input
                                id="post-search-q"
                                type="search"
                                value={keywordInput}
                                onChange={(event) => setKeywordInput(event.target.value)}
                                onBlur={() => {
                                    const normalized = keywordInput.trim() || undefined;
                                    if (normalized !== search.q) {
                                        updateSearch({ q: normalized });
                                    }
                                }}
                                placeholder="按标题或 slug 搜索…"
                                className="studio-input h-9 w-full pl-8.5 text-[13.5px]"
                            />
                        </div>

                        <Select
                            id="post-filter-review"
                            ariaLabel="审核状态"
                            value={search.review_status ?? ''}
                            onChange={(nextValue) =>
                                updateSearch({
                                    review_status: (nextValue || undefined) as
                                        | Schemas['ReviewStatus']
                                        | undefined,
                                })
                            }
                            options={[
                                { value: '', label: '全部审核状态' },
                                { value: 'pending', label: '审核中（待审核）' },
                                { value: 'approved', label: '已批准' },
                                { value: 'rejected', label: '已拒绝' },
                            ]}
                        />

                        <Select
                            id="post-filter-published"
                            ariaLabel="线上状态"
                            value={
                                search.published === undefined
                                    ? ''
                                    : search.published
                                      ? 'true'
                                      : 'false'
                            }
                            onChange={(nextValue) =>
                                updateSearch({
                                    published: nextValue === '' ? undefined : nextValue === 'true',
                                })
                            }
                            options={[
                                { value: '', label: '全部线上状态' },
                                { value: 'true', label: '已上线' },
                                { value: 'false', label: '未上线' },
                            ]}
                        />

                        <Select
                            id="post-filter-category"
                            ariaLabel="分类"
                            value={search.category_id ?? ''}
                            onChange={(nextValue) =>
                                updateSearch({ category_id: nextValue || undefined })
                            }
                            options={[
                                { value: '', label: '全部分类' },
                                ...(categoriesQuery.data?.data ?? []).map((cat) => ({
                                    value: cat.id,
                                    label: cat.name,
                                })),
                            ]}
                        />

                        <Select
                            id="post-filter-tag"
                            ariaLabel="标签"
                            value={search.tag_id ?? ''}
                            onChange={(nextValue) =>
                                updateSearch({ tag_id: nextValue || undefined })
                            }
                            options={[
                                { value: '', label: '全部标签' },
                                ...(tagsQuery.data?.data ?? []).map((tag) => ({
                                    value: tag.id,
                                    label: tag.name,
                                })),
                            ]}
                        />

                        {canReview ? (
                            <button
                                type="button"
                                aria-pressed={search.review_status === 'pending'}
                                onClick={() =>
                                    updateSearch({
                                        review_status:
                                            search.review_status === 'pending'
                                                ? undefined
                                                : 'pending',
                                    })
                                }
                                className={cn(
                                    'h-9 rounded-[9px] border px-3 text-[13px] font-medium transition-colors',
                                    search.review_status === 'pending'
                                        ? 'border-warn/30 bg-warn-tint text-warn'
                                        : 'border-ui-line bg-white text-ui-sub hover:border-ui-line-strong hover:text-ui-text',
                                )}
                            >
                                只看待审核
                            </button>
                        ) : null}

                        {hasFilters ? (
                            <Button
                                variant="ghost"
                                size="sm"
                                onClick={() => {
                                    setKeywordInput('');
                                    void navigate({ search: {} });
                                }}
                            >
                                <X className="h-3.5 w-3.5" />
                                清除筛选
                            </Button>
                        ) : null}
                    </form>
                </search>

                {postsQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            title="文章列表加载失败"
                            error={postsQuery.error}
                            onRetry={() => void postsQuery.refetch()}
                        />
                    </div>
                ) : postsQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载文章列表…" />
                    </div>
                ) : posts.length === 0 ? (
                    <EmptyState
                        className="rounded-none border-0 shadow-none"
                        icon={<FileText className="h-5 w-5" />}
                        title="没有符合条件的文章"
                        description={
                            hasFilters
                                ? '尝试调整或清除当前筛选条件。'
                                : '新建你的第一篇文章开始创作。'
                        }
                        action={
                            <>
                                {hasFilters ? (
                                    <Button
                                        variant="secondary"
                                        onClick={() => {
                                            setKeywordInput('');
                                            void navigate({ search: {} });
                                        }}
                                    >
                                        清除筛选
                                    </Button>
                                ) : null}
                                {canCreate ? (
                                    <Link
                                        to="/posts/new"
                                        className="inline-flex h-9 items-center gap-1.5 rounded-[9px] bg-accent px-4 text-[13.5px] font-medium text-white hover:bg-accent-2"
                                    >
                                        <FilePlus2 className="h-4 w-4" />
                                        新建文章
                                    </Link>
                                ) : null}
                            </>
                        }
                    />
                ) : (
                    <>
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[860px]">
                                <thead>
                                    <tr>
                                        <th className="w-[34%]">标题与 Slug</th>
                                        <th>编辑状态</th>
                                        <th>期望公开</th>
                                        <th>当前线上</th>
                                        {canManageAll ? <th>作者</th> : null}
                                        <th>更新时间</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {posts.map((post) => {
                                        const editMeta = reviewStatusChip(
                                            post.current_revision_no,
                                            post.current_review,
                                        );
                                        const isPendingReview =
                                            canReview && post.current_review?.status === 'pending';

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
                                                        {post.slug_locked ? ' · 已锁定' : ''}
                                                    </div>
                                                </td>
                                                <td>
                                                    <StatusChip tone={editMeta.tone}>
                                                        {editMeta.label}
                                                    </StatusChip>
                                                </td>
                                                <td>{renderDesiredColumn(post)}</td>
                                                <td>
                                                    {post.live_revision_id ? (
                                                        <StatusChip tone="ok">已上线</StatusChip>
                                                    ) : (
                                                        <StatusChip tone="mute">未上线</StatusChip>
                                                    )}
                                                </td>
                                                {canManageAll ? (
                                                    <td className="text-ui-text">
                                                        {post.author?.display_name ?? '—'}
                                                    </td>
                                                ) : null}
                                                <td className="tnum whitespace-nowrap text-[12.5px] text-ui-sub">
                                                    {formatShortDateTime(post.updated_at)}
                                                </td>
                                                <td className="whitespace-nowrap text-right">
                                                    <div className="inline-flex items-center gap-2">
                                                        {isPendingReview ? (
                                                            <Link
                                                                to="/posts/$postId/revisions/$revisionId"
                                                                params={{
                                                                    postId: post.id,
                                                                    revisionId:
                                                                        post.current_revision_id,
                                                                }}
                                                                className="rounded-md bg-accent-tint px-2.5 py-1 text-[12.5px] font-medium text-accent-2 hover:bg-accent hover:text-white"
                                                            >
                                                                审核 r{post.current_revision_no}
                                                            </Link>
                                                        ) : (
                                                            <Link
                                                                to="/posts/$postId/edit"
                                                                params={{ postId: post.id }}
                                                                className="rounded-md px-2 py-1 text-[13px] font-medium text-accent hover:bg-accent-tint"
                                                            >
                                                                编辑
                                                            </Link>
                                                        )}
                                                        <Link
                                                            to="/posts/$postId/revisions"
                                                            params={{ postId: post.id }}
                                                            className="rounded-md px-2 py-1 text-[13px] text-ui-sub hover:bg-ui-bg hover:text-ui-text"
                                                        >
                                                            历史
                                                        </Link>
                                                    </div>
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
                                unit="篇"
                                onPageChange={(nextPage) => updateSearch({ page: nextPage })}
                            />
                        ) : null}
                    </>
                )}
            </div>
        </PageContainer>
    );
}
