import { postPublicationStatus } from '@/features/posts/post-publication-status';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import { ArrowRight, BookOpen, FileText, Plus, Search, Sparkles, X } from 'lucide-react';
import { useMemo, useState } from 'react';
import { completionStatusMeta, isValidSeriesSlug, seriesPublishStatusMeta } from './series-status';
import { AuthenticatedImage } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import {
    EmptyState,
    ErrorBanner,
    LoadingCard,
    PageContainer,
    PageHeader,
    PaginationBar,
} from '@/components/ui/feedback';
import { Drawer } from '@/components/ui/overlay';
import { Select } from '@/components/ui/select';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { usePostDeletion } from '@/features/posts/use-post-deletion';
import { apiClient, unwrapResponse } from '@/lib/api';
import { formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { formatShortDateTime } from '@/lib/format';
import { hasAnyPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export type SeriesSort = 'sort_order_asc' | 'updated_desc' | 'created_desc';

export interface SeriesSearchParams {
    page?: number;
    per_page?: 20 | 50 | 100;
    q?: string;
    category_id?: string;
    published?: boolean;
    completion_status?: 'ongoing' | 'completed';
    sort?: SeriesSort;
}

interface CreateSeriesForm {
    title: string;
    slug: string;
    category_id: string;
    description: string;
    completion_status: 'ongoing' | 'completed';
    is_featured: boolean;
    sort_order: string;
}

const INITIAL_CREATE_FORM: CreateSeriesForm = {
    title: '',
    slug: '',
    category_id: '',
    description: '',
    completion_status: 'ongoing',
    is_featured: false,
    sort_order: '0',
};

export function SeriesListPage() {
    const { user } = useAuth();
    const { toast } = useToast();
    const navigate = useNavigate({ from: '/series' });
    const search = useSearch({ from: '/_authed/series' });
    const queryClient = useQueryClient();

    const deletion = usePostDeletion();
    const canViewPosts = hasAnyPermission(user, [
        PERMISSIONS.POSTS_READ_OWN,
        PERMISSIONS.POSTS_MANAGE,
    ]);

    const page = search.page ?? 1;
    const perPage: 20 | 50 | 100 = search.per_page ?? 20;

    const [keywordDraft, setKeywordDraft] = useState<{ base: string; value: string }>({
        base: search.q ?? '',
        value: search.q ?? '',
    });
    const keywordInput =
        keywordDraft.base === (search.q ?? '') ? keywordDraft.value : (search.q ?? '');
    const setKeywordInput = (value: string) => setKeywordDraft({ base: search.q ?? '', value });

    const [createOpen, setCreateOpen] = useState(false);
    const [createForm, setCreateForm] = useState<CreateSeriesForm>(INITIAL_CREATE_FORM);
    const [createError, setCreateError] = useState<string | null>(null);

    const updateSearch = (patch: Partial<SeriesSearchParams>) => {
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

    const queryParams = {
        page,
        per_page: perPage,
        q: search.q || undefined,
        category_id: search.category_id || undefined,
        published: search.published,
        completion_status: search.completion_status || undefined,
        sort: search.sort || undefined,
    };

    const seriesQuery = useQuery({
        queryKey: queryKeys.series.list(queryParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/series', {
                    params: { query: queryParams },
                }),
            ),
    });

    const standalonePostsQuery = useQuery({
        queryKey: queryKeys.posts.list({ in_series: false, page: 1, per_page: 10 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: { query: { in_series: false, page: 1, per_page: 10 } },
                }),
            ),
        enabled: canViewPosts,
    });

    const createMutation = useMutation({
        mutationFn: async (form: CreateSeriesForm) => {
            const sortOrder = Number.parseInt(form.sort_order.trim() || '0', 10);
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/series', {
                    body: {
                        title: form.title.trim(),
                        slug: form.slug.trim(),
                        description: form.description.trim() || null,
                        category_id: form.category_id || null,
                        completion_status: form.completion_status,
                        is_featured: form.is_featured,
                        sort_order: Number.isFinite(sortOrder) ? sortOrder : 0,
                    },
                }),
            );
            return res.data;
        },
        onSuccess: (created) => {
            setCreateOpen(false);
            setCreateForm(INITIAL_CREATE_FORM);
            setCreateError(null);
            toast(`系列「${created.title}」已创建`, 'ok');
            queryClient.setQueryData(queryKeys.series.detail(created.id), created);
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            void navigate({
                to: '/series/$seriesId',
                params: { seriesId: created.id },
            });
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'slug_in_use') {
                setCreateError('该 Slug 已被其他系列占用，请更换后再试。');
            } else {
                setCreateError(formatErrorWithRequestId(error));
            }
        },
    });

    const categories = useMemo(
        () => categoriesQuery.data?.data ?? [],
        [categoriesQuery.data?.data],
    );
    const categoryNameById = useMemo(
        () => new Map(categories.map((cat) => [cat.id, cat.name] as const)),
        [categories],
    );

    const items = seriesQuery.data?.data ?? [];
    const pagination = seriesQuery.data?.meta.pagination;
    const standalonePosts = standalonePostsQuery.data?.data ?? [];
    const standaloneTotal = standalonePostsQuery.data?.meta.pagination.total ?? 0;

    const hasFilters = Boolean(
        search.q ||
        search.category_id ||
        search.published !== undefined ||
        search.completion_status ||
        (search.sort && search.sort !== 'sort_order_asc'),
    );

    const handleCreateSubmit = (event: React.FormEvent) => {
        event.preventDefault();
        if (!createForm.title.trim()) {
            setCreateError('请输入系列标题（1–100 字符）。');
            return;
        }
        if (createForm.title.trim().length > 100) {
            setCreateError('系列标题不能超过 100 个字符。');
            return;
        }
        if (!isValidSeriesSlug(createForm.slug)) {
            setCreateError(
                'Slug 仅允许小写字母、数字和单个连字符，不能以连字符开头或结尾（1–100 字符）。',
            );
            return;
        }
        setCreateError(null);
        createMutation.mutate(createForm);
    };

    return (
        <PageContainer size="fluid" className="max-w-[1440px] space-y-5">
            <PageHeader
                eyebrow="BLOG · SERIES"
                title="专栏管理"
                description="以专栏为入口组织教程目录、新建与编辑章节文章。未归属专栏的独立文章会在下方自动列出。"
                actions={
                    <>
                        {canViewPosts ? (
                            <Link
                                to="/posts"
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                <FileText className="h-4 w-4 text-ui-sub" />
                                全部文章
                            </Link>
                        ) : null}
                        <Button
                            variant="primary"
                            onClick={() => {
                                setCreateError(null);
                                setCreateOpen(true);
                            }}
                        >
                            <Plus className="h-4 w-4" />
                            新建专栏
                        </Button>
                    </>
                }
            />

            <div className="studio-card overflow-hidden">
                <search
                    aria-label="筛选文章系列"
                    className="border-b border-ui-line bg-ui-subtle/70 p-3.5"
                >
                    <form
                        onSubmit={(event) => {
                            event.preventDefault();
                            updateSearch({ q: keywordInput.trim() || undefined });
                        }}
                        className="flex flex-wrap items-center gap-2.5"
                    >
                        <div className="relative min-w-[200px] flex-1 sm:max-w-[260px]">
                            <Search
                                aria-hidden="true"
                                className="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[#889B98]"
                            />
                            <label className="sr-only" htmlFor="series-search-q">
                                搜索标题或 Slug
                            </label>
                            <input
                                id="series-search-q"
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
                            id="series-filter-category"
                            ariaLabel="主分类"
                            value={search.category_id ?? ''}
                            onChange={(nextValue) =>
                                updateSearch({ category_id: nextValue || undefined })
                            }
                            options={[
                                { value: '', label: '全部主分类' },
                                ...categories.map((cat) => ({
                                    value: cat.id,
                                    label: cat.name,
                                })),
                            ]}
                        />

                        <Select
                            id="series-filter-published"
                            ariaLabel="公开状态"
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
                                { value: '', label: '全部公开状态' },
                                { value: 'true', label: '已发布' },
                                { value: 'false', label: '未公开' },
                            ]}
                        />

                        <Select
                            id="series-filter-completion"
                            ariaLabel="连载状态"
                            value={search.completion_status ?? ''}
                            onChange={(nextValue) =>
                                updateSearch({
                                    completion_status: (nextValue || undefined) as
                                        | 'ongoing'
                                        | 'completed'
                                        | undefined,
                                })
                            }
                            options={[
                                { value: '', label: '全部连载状态' },
                                { value: 'ongoing', label: '连载中' },
                                { value: 'completed', label: '已完结' },
                            ]}
                        />

                        <Select
                            id="series-filter-sort"
                            ariaLabel="排序方式"
                            value={
                                search.sort && search.sort !== 'sort_order_asc' ? search.sort : ''
                            }
                            onChange={(nextValue) =>
                                updateSearch({
                                    sort: nextValue ? (nextValue as SeriesSort) : undefined,
                                })
                            }
                            options={[
                                { value: '', label: '按排序权重' },
                                { value: 'updated_desc', label: '最近更新' },
                                { value: 'created_desc', label: '最近创建' },
                            ]}
                        />

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

                {seriesQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            title="专栏列表加载失败"
                            error={seriesQuery.error}
                            onRetry={() => void seriesQuery.refetch()}
                        />
                    </div>
                ) : seriesQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载专栏…" />
                    </div>
                ) : items.length === 0 ? (
                    <EmptyState
                        className="rounded-none border-0 shadow-none"
                        icon={<BookOpen className="h-5 w-5" />}
                        title="还没有专栏"
                        description={
                            hasFilters
                                ? '尝试调整或清除当前筛选条件。'
                                : '创建一个专栏草稿，开始编排有序的目录与文章。'
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
                                <Button variant="primary" onClick={() => setCreateOpen(true)}>
                                    <Plus className="h-4 w-4" />
                                    新建专栏
                                </Button>
                            </>
                        }
                    />
                ) : (
                    <>
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[920px]">
                                <thead>
                                    <tr>
                                        <th className="w-[34%]">专栏与 Slug</th>
                                        <th>主分类</th>
                                        <th>连载状态</th>
                                        <th>公开与线上状态</th>
                                        <th>文章进度</th>
                                        <th>权重</th>
                                        <th>更新时间</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {items.map((item) => {
                                        const pubMeta = seriesPublishStatusMeta(item);
                                        const compMeta = completionStatusMeta(
                                            item.completion_status,
                                        );
                                        const categoryName = item.category_id
                                            ? categoryNameById.get(item.category_id)
                                            : null;

                                        return (
                                            <tr key={item.id}>
                                                <td aria-label={item.title}>
                                                    <div className="flex items-center gap-3">
                                                        <div className="h-11 w-16 shrink-0 overflow-hidden rounded-lg border border-ui-line bg-[#EEF3F2]">
                                                            {item.cover_media_id ? (
                                                                <AuthenticatedImage
                                                                    mediaId={item.cover_media_id}
                                                                    alt={item.title}
                                                                    className="h-full w-full object-cover"
                                                                />
                                                            ) : (
                                                                <div className="grid h-full w-full place-items-center text-ui-sub">
                                                                    <BookOpen className="h-4 w-4 opacity-50" />
                                                                </div>
                                                            )}
                                                        </div>
                                                        <div className="min-w-0">
                                                            <div className="flex items-center gap-1.5">
                                                                <Link
                                                                    to="/series/$seriesId"
                                                                    params={{ seriesId: item.id }}
                                                                    className="truncate font-medium text-ui-text transition-colors hover:text-accent"
                                                                >
                                                                    {item.title}
                                                                </Link>
                                                                {item.is_featured ? (
                                                                    <span
                                                                        title="精选系列"
                                                                        className="inline-flex shrink-0 items-center gap-0.5 rounded-full bg-warn-tint px-1.5 py-0.5 text-[10.5px] font-medium text-warn"
                                                                    >
                                                                        <Sparkles className="h-2.5 w-2.5" />
                                                                        精选
                                                                    </span>
                                                                ) : null}
                                                            </div>
                                                            <div className="mt-0.5 truncate font-code text-[11.5px] text-ui-sub">
                                                                /series/{item.slug}/
                                                                {item.slug_locked
                                                                    ? ' · 已锁定'
                                                                    : ''}
                                                            </div>
                                                        </div>
                                                    </div>
                                                </td>
                                                <td>
                                                    {categoryName ? (
                                                        <span className="inline-flex items-center rounded-md border border-ui-line bg-ui-subtle px-2 py-0.5 text-[12px] font-medium text-ui-text">
                                                            {categoryName}
                                                        </span>
                                                    ) : (
                                                        <span className="text-[12.5px] text-ui-sub">
                                                            未设置
                                                        </span>
                                                    )}
                                                </td>
                                                <td>
                                                    <StatusChip tone={compMeta.tone}>
                                                        {compMeta.label}
                                                    </StatusChip>
                                                </td>
                                                <td>
                                                    <StatusChip tone={pubMeta.tone}>
                                                        {pubMeta.label}
                                                    </StatusChip>
                                                </td>
                                                <td className="tnum text-[13px]">
                                                    <span className="font-medium text-ui-text">
                                                        已发布 {item.published_post_count}
                                                    </span>
                                                    <span className="text-ui-sub">
                                                        {' '}
                                                        / 目录共 {item.item_count} 篇
                                                    </span>
                                                </td>
                                                <td className="tnum font-code text-[13px] text-ui-sub">
                                                    {item.sort_order}
                                                </td>
                                                <td className="tnum whitespace-nowrap text-[12.5px] text-ui-sub">
                                                    {formatShortDateTime(item.updated_at)}
                                                </td>
                                                <td className="whitespace-nowrap text-right">
                                                    <Link
                                                        to="/series/$seriesId"
                                                        params={{ seriesId: item.id }}
                                                        className="rounded-md px-2.5 py-1 text-[13px] font-medium text-accent hover:bg-accent-tint"
                                                    >
                                                        进入工作台
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
                                unit="个专栏"
                                perPageOptions={[20, 50, 100]}
                                onPerPageChange={(nextPerPage) =>
                                    updateSearch({
                                        per_page: nextPerPage as 20 | 50 | 100,
                                        page: 1,
                                    })
                                }
                                onPageChange={(nextPage) => updateSearch({ page: nextPage })}
                            />
                        ) : null}
                    </>
                )}
            </div>

            {canViewPosts && standalonePosts.length > 0 ? (
                <section
                    aria-labelledby="standalone-admin-posts-h"
                    className="studio-card overflow-hidden"
                >
                    <header className="flex flex-wrap items-center justify-between gap-2 border-b border-ui-line bg-ui-subtle/60 px-5 py-3">
                        <div className="flex items-center gap-2">
                            <h2
                                id="standalone-admin-posts-h"
                                className="text-[14.5px] font-semibold text-ui-text"
                            >
                                未归属专栏的独立文章
                            </h2>
                            <span className="tnum rounded-full bg-white px-2 py-0.5 text-[11.5px] font-medium text-ui-sub shadow-2xs">
                                {standaloneTotal} 篇
                            </span>
                        </div>
                        <Link
                            to="/posts"
                            search={{ in_series: false }}
                            className="inline-flex items-center gap-1 text-[13px] font-medium text-accent hover:underline"
                        >
                            查看全部独立文章
                            <ArrowRight className="h-3.5 w-3.5" />
                        </Link>
                    </header>
                    <div className="overflow-x-auto">
                        <table className="tbl min-w-[760px]">
                            <thead>
                                <tr>
                                    <th className="w-[42%]">标题与 Slug</th>
                                    <th>发布状态</th>
                                    <th>更新时间</th>
                                    <th className="text-right">操作</th>
                                </tr>
                            </thead>
                            <tbody>
                                {standalonePosts.map((post) => {
                                    const status = postPublicationStatus(post);
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
                                                <StatusChip tone={status.tone}>
                                                    {status.label}
                                                </StatusChip>
                                            </td>
                                            <td className="tnum whitespace-nowrap text-[12.5px] text-ui-sub">
                                                {formatShortDateTime(post.updated_at)}
                                            </td>
                                            <td className="whitespace-nowrap text-right">
                                                <Link
                                                    to="/posts/$postId/edit"
                                                    params={{ postId: post.id }}
                                                    className="rounded-md px-2 py-1 text-[13px] font-medium text-accent hover:bg-accent-tint"
                                                >
                                                    编辑
                                                </Link>
                                                {deletion.renderButton(post)}
                                            </td>
                                        </tr>
                                    );
                                })}
                            </tbody>
                        </table>
                    </div>
                </section>
            ) : null}

            {deletion.dialog}
            <Drawer
                open={createOpen}
                onClose={() => setCreateOpen(false)}
                title="新建专栏"
                subtitle="创建为未公开草稿，可在工作台继续完善封面、目录与发布"
                widthClassName="sm:w-[460px]"
                footer={
                    <div className="flex justify-end gap-2">
                        <Button variant="secondary" onClick={() => setCreateOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            type="submit"
                            form="create-series-form"
                            disabled={createMutation.isPending}
                        >
                            {createMutation.isPending ? '创建中…' : '创建并进入工作台'}
                        </Button>
                    </div>
                }
            >
                <form
                    id="create-series-form"
                    onSubmit={handleCreateSubmit}
                    className="space-y-4 text-[14px]"
                >
                    {createError ? (
                        <div
                            role="alert"
                            className="rounded-lg border border-bad/30 bg-bad-tint px-3.5 py-2.5 text-[13px] text-bad"
                        >
                            {createError}
                        </div>
                    ) : null}

                    <div>
                        <label htmlFor="create-series-title" className="block font-medium">
                            系列标题 <span className="text-bad">*</span>
                        </label>
                        <input
                            id="create-series-title"
                            type="text"
                            maxLength={100}
                            value={createForm.title}
                            onChange={(event) =>
                                setCreateForm((prev) => ({ ...prev, title: event.target.value }))
                            }
                            placeholder="例如：Rust 并发编程"
                            className="studio-input mt-1.5"
                            required
                        />
                    </div>

                    <div>
                        <label htmlFor="create-series-slug" className="block font-medium">
                            Slug <span className="text-bad">*</span>
                        </label>
                        <input
                            id="create-series-slug"
                            type="text"
                            maxLength={100}
                            value={createForm.slug}
                            onChange={(event) =>
                                setCreateForm((prev) => ({ ...prev, slug: event.target.value }))
                            }
                            placeholder="rust-concurrency"
                            className="studio-input mt-1.5 font-code text-[13px]"
                            required
                        />
                        <p className="mt-1 text-[12px] text-ui-sub">
                            公开路径为{' '}
                            <span className="font-code">/series/{createForm.slug || 'slug'}/</span>
                            ，首次公开后将锁定不可修改。
                        </p>
                    </div>

                    <div>
                        <label htmlFor="create-series-category" className="block font-medium">
                            所属主分类
                        </label>
                        <div className="mt-1.5">
                            <Select
                                id="create-series-category"
                                ariaLabel="所属主分类"
                                className="w-full"
                                value={createForm.category_id}
                                onChange={(nextValue) =>
                                    setCreateForm((prev) => ({ ...prev, category_id: nextValue }))
                                }
                                options={[
                                    { value: '', label: '不关联主分类' },
                                    ...categories.map((cat) => ({
                                        value: cat.id,
                                        label: cat.name,
                                    })),
                                ]}
                            />
                        </div>
                    </div>

                    <div className="grid grid-cols-2 gap-3">
                        <div>
                            <label htmlFor="create-series-status" className="block font-medium">
                                连载状态
                            </label>
                            <div className="mt-1.5">
                                <Select
                                    id="create-series-status"
                                    ariaLabel="连载状态"
                                    className="w-full"
                                    value={createForm.completion_status}
                                    onChange={(nextValue) =>
                                        setCreateForm((prev) => ({
                                            ...prev,
                                            completion_status: nextValue as 'ongoing' | 'completed',
                                        }))
                                    }
                                    options={[
                                        { value: 'ongoing', label: '连载中' },
                                        { value: 'completed', label: '已完结' },
                                    ]}
                                />
                            </div>
                        </div>
                        <div>
                            <label htmlFor="create-series-order" className="block font-medium">
                                排序权重
                            </label>
                            <input
                                id="create-series-order"
                                type="number"
                                value={createForm.sort_order}
                                onChange={(event) =>
                                    setCreateForm((prev) => ({
                                        ...prev,
                                        sort_order: event.target.value,
                                    }))
                                }
                                className="studio-input mt-1.5 font-code text-[13px]"
                            />
                        </div>
                    </div>

                    <div>
                        <div className="flex justify-between">
                            <label htmlFor="create-series-desc" className="block font-medium">
                                系列简介
                            </label>
                            <span className="tnum text-[12px] text-ui-sub">
                                {createForm.description.length}/1000
                            </span>
                        </div>
                        <textarea
                            id="create-series-desc"
                            rows={3}
                            maxLength={1000}
                            value={createForm.description}
                            onChange={(event) =>
                                setCreateForm((prev) => ({
                                    ...prev,
                                    description: event.target.value,
                                }))
                            }
                            placeholder="说明本系列的目标读者、核心内容与阅读收获（可选）"
                            className="studio-input mt-1.5 h-auto py-2 text-[13px]"
                        />
                    </div>

                    <label
                        htmlFor="create-series-featured"
                        aria-label="精选系列"
                        className="flex items-start gap-2.5 pt-1"
                    >
                        <input
                            id="create-series-featured"
                            type="checkbox"
                            checked={createForm.is_featured}
                            onChange={(event) =>
                                setCreateForm((prev) => ({
                                    ...prev,
                                    is_featured: event.target.checked,
                                }))
                            }
                            className="mt-0.5 h-4 w-4 accent-[#137A4D]"
                        />
                        <span>
                            <span className="font-medium text-ui-text">精选系列</span>
                            <span className="block text-[12px] text-ui-sub">
                                在公开站点系列总览卡片上显示「精选」标记。
                            </span>
                        </span>
                    </label>
                </form>
            </Drawer>
        </PageContainer>
    );
}
