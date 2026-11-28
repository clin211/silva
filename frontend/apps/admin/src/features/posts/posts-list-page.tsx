import { postPublicationStatus } from './post-publication-status';
import type { Schemas } from '@silva/api-client';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import {
    AlertCircle,
    ArrowUpDown,
    BookOpen,
    FilePlus2,
    FileText,
    FolderOpen,
    Inbox,
    Layers,
    PanelLeftClose,
    PanelLeftOpen,
    Search,
    X,
} from 'lucide-react';
import { useMemo, useState } from 'react';
import { usePostDeletion } from './use-post-deletion';
import { Button } from '@/components/ui/button';
import {
    EmptyState,
    ErrorBanner,
    LoadingCard,
    PageContainer,
    PageHeader,
    PaginationBar,
} from '@/components/ui/feedback';
import { Select } from '@/components/ui/select';
import { StatusChip } from '@/components/ui/status-chip';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, unwrapResponse } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatShortDateTime } from '@/lib/format';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export interface PostsSearchParams {
    page?: number;
    per_page?: 20 | 50 | 100;
    scope?: 'mine' | 'all';
    q?: string;
    published?: boolean;
    category_id?: string;
    uncategorized?: boolean;
    tag_id?: string;
    series_id?: string;
    in_series?: boolean;
    sort?: Schemas['PostSort'];
}

const CAT_SIDEBAR_STORAGE_KEY = 'silva_admin_posts_cat_sidebar_collapsed';

function readSidebarCollapsed(): boolean {
    try {
        return window.localStorage.getItem(CAT_SIDEBAR_STORAGE_KEY) === '1';
    } catch {
        return false;
    }
}

function writeSidebarCollapsed(collapsed: boolean): void {
    try {
        window.localStorage.setItem(CAT_SIDEBAR_STORAGE_KEY, collapsed ? '1' : '0');
    } catch {
        // Ignore storage quota / privacy errors
    }
}

export function PostsListPage() {
    const { user } = useAuth();
    const navigate = useNavigate({ from: '/posts' });
    const search = useSearch({ from: '/_authed/posts' });

    const canManageAll = hasPermission(user, PERMISSIONS.POSTS_MANAGE);
    const canCreate = hasPermission(user, PERMISSIONS.POSTS_CREATE);
    const canManageSeries = hasPermission(user, PERMISSIONS.SERIES_MANAGE);

    const effectiveScope: 'mine' | 'all' = !canManageAll ? 'mine' : (search.scope ?? 'all');
    const page = search.page ?? 1;
    const perPage: 20 | 50 | 100 = search.per_page ?? 20;

    const [sidebarCollapsed, setSidebarCollapsed] = useState<boolean>(readSidebarCollapsed);
    const [catSortMode, setCatSortMode] = useState<'count' | 'name'>('count');
    const [catKeyword, setCatKeyword] = useState('');

    const [keywordDraft, setKeywordDraft] = useState<{ base: string; value: string }>({
        base: search.q ?? '',
        value: search.q ?? '',
    });
    const keywordInput =
        keywordDraft.base === (search.q ?? '') ? keywordDraft.value : (search.q ?? '');
    const setKeywordInput = (value: string) => setKeywordDraft({ base: search.q ?? '', value });

    const toggleSidebar = () => {
        setSidebarCollapsed((prev) => {
            const next = !prev;
            writeSidebarCollapsed(next);
            return next;
        });
    };

    const updateSearch = (patch: Partial<PostsSearchParams>) => {
        void navigate({
            search: (prev) => ({
                ...prev,
                ...patch,
                page: patch.page ?? 1,
            }),
        });
    };

    const selectAllCategories = () =>
        updateSearch({ category_id: undefined, uncategorized: undefined, page: 1 });

    const selectUncategorized = () =>
        updateSearch({ category_id: undefined, uncategorized: true, page: 1 });

    const selectCategory = (categoryId: string) =>
        updateSearch({ category_id: categoryId, uncategorized: undefined, page: 1 });

    const effectiveSeriesId = canManageSeries ? search.series_id || undefined : undefined;
    const effectiveInSeries = canManageSeries && !effectiveSeriesId ? search.in_series : undefined;

    const categoryCountsParams = {
        author_id: effectiveScope === 'mine' ? user?.id : undefined,
        series_id: effectiveSeriesId,
        in_series: effectiveInSeries,
    };

    const categoryCountsQuery = useQuery({
        queryKey: queryKeys.posts.categoryCounts(categoryCountsParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/category-counts', {
                    params: { query: categoryCountsParams },
                }),
            ),
    });

    const categoriesQuery = useQuery({
        queryKey: queryKeys.taxonomy.categories,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/categories')),
    });

    const tagsQuery = useQuery({
        queryKey: queryKeys.taxonomy.tags,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/tags')),
    });

    const seriesListQuery = useQuery({
        queryKey: queryKeys.series.list({ page: 1, per_page: 100 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/series', {
                    params: { query: { page: 1, per_page: 100 } },
                }),
            ),
        enabled: canManageSeries,
    });

    const queryParams = {
        page,
        per_page: perPage,
        q: search.q || undefined,
        author_id: effectiveScope === 'mine' ? user?.id : undefined,
        category_id: search.uncategorized ? undefined : search.category_id || undefined,
        uncategorized: search.uncategorized || undefined,
        tag_id: search.tag_id || undefined,
        series_id: effectiveSeriesId,
        in_series: effectiveInSeries,
        published: search.published,
        sort: search.sort || undefined,
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

    const countsData = categoryCountsQuery.data?.data;

    const directoryCategories = useMemo<Schemas['PostCategoryCount'][]>(() => {
        const source: Schemas['PostCategoryCount'][] =
            countsData?.categories ??
            (categoriesQuery.data?.data ?? []).map((cat) => ({
                id: cat.id,
                slug: cat.slug,
                name: cat.name,
                description: cat.description,
                post_count: cat.post_count,
            }));

        return [...source].sort((a, b) => {
            if (catSortMode === 'count') {
                if (b.post_count !== a.post_count) return b.post_count - a.post_count;
                return a.name.localeCompare(b.name, 'zh-CN');
            }
            return a.name.localeCompare(b.name, 'zh-CN');
        });
    }, [countsData?.categories, categoriesQuery.data?.data, catSortMode]);

    const normalizedCatKeyword = catKeyword.trim().toLowerCase();
    const filteredDirectoryCategories = useMemo(() => {
        if (!normalizedCatKeyword) return directoryCategories;
        return directoryCategories.filter(
            (cat) =>
                cat.name.toLowerCase().includes(normalizedCatKeyword) ||
                cat.slug.toLowerCase().includes(normalizedCatKeyword),
        );
    }, [directoryCategories, normalizedCatKeyword]);

    const isAllCategoriesActive = !search.category_id && !search.uncategorized;
    const isUncategorizedActive = search.uncategorized === true;
    const selectedCategory = useMemo(
        () =>
            search.category_id
                ? directoryCategories.find((cat) => cat.id === search.category_id)
                : undefined,
        [directoryCategories, search.category_id],
    );

    const posts = postsQuery.data?.data ?? [];
    const deletion = usePostDeletion(() => {
        if (page > 1 && posts.length === 1) updateSearch({ page: page - 1 });
    });
    const pagination = postsQuery.data?.meta.pagination;

    const hasFilters = Boolean(
        search.q ||
        search.published !== undefined ||
        search.category_id ||
        search.uncategorized ||
        search.tag_id ||
        (canManageSeries && (search.series_id || search.in_series !== undefined)) ||
        (canManageAll && effectiveScope === 'mine'),
    );

    const activeViewTitle = isUncategorizedActive
        ? '未分类'
        : selectedCategory
          ? selectedCategory.name
          : search.category_id
            ? '指定分类'
            : '全部文章';

    const activeViewDescription = isUncategorizedActive
        ? '尚未分配任何分类的文章'
        : (selectedCategory?.description ?? null);

    const activeViewCount =
        pagination?.total ??
        (isUncategorizedActive
            ? countsData?.uncategorized
            : selectedCategory
              ? selectedCategory.post_count
              : countsData?.total);

    const createLinkSearch = search.category_id ? { category_id: search.category_id } : {};

    const uncategorizedCount = countsData?.uncategorized;
    const totalCount = countsData?.total;
    const directoryHasNoData = directoryCategories.length === 0;

    return (
        <PageContainer size="fluid" className="max-w-[1520px] space-y-5">
            <PageHeader
                eyebrow="CONTENT"
                title="文章管理"
                actions={
                    <>
                        {canManageSeries ? (
                            <Link
                                to="/series"
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                <BookOpen className="h-4 w-4 text-ui-sub" />
                                返回专栏管理
                            </Link>
                        ) : null}
                        <Button
                            variant="secondary"
                            size="sm"
                            onClick={toggleSidebar}
                            aria-pressed={!sidebarCollapsed}
                            className="hidden h-9 lg:inline-flex"
                        >
                            {sidebarCollapsed ? (
                                <>
                                    <PanelLeftOpen className="h-4 w-4" />
                                    展开分类栏
                                </>
                            ) : (
                                <>
                                    <PanelLeftClose className="h-4 w-4" />
                                    收起分类栏
                                </>
                            )}
                        </Button>
                        {canCreate ? (
                            <Link
                                to="/posts/new"
                                search={createLinkSearch}
                                className="inline-flex h-9 items-center gap-1.5 rounded-[9px] bg-accent px-4 text-[13.5px] font-medium text-white shadow-[0_1px_2px_rgba(8,127,137,0.28)] transition-colors hover:bg-accent-2"
                            >
                                <FilePlus2 className="h-4 w-4" />
                                新建文章
                            </Link>
                        ) : null}
                    </>
                }
            />

            <div
                className={cn(
                    'grid grid-cols-1 gap-5',
                    !sidebarCollapsed && 'lg:grid-cols-[248px_minmax(0,1fr)] lg:items-start',
                )}
            >
                {/* 左侧分类目录栏（桌面端） */}
                {!sidebarCollapsed ? (
                    <aside
                        aria-label="分类目录"
                        className="studio-card hidden overflow-hidden lg:sticky lg:top-[82px] lg:flex lg:max-h-[calc(100vh-108px)] lg:flex-col"
                    >
                        <div className="flex items-center justify-between gap-2 border-b border-ui-line bg-ui-subtle/60 px-3.5 py-2.5">
                            <h2 className="text-[13px] font-semibold text-ui-text">分类目录</h2>
                            <button
                                type="button"
                                onClick={() =>
                                    setCatSortMode((prev) => (prev === 'count' ? 'name' : 'count'))
                                }
                                title={
                                    catSortMode === 'count'
                                        ? '当前按文章数降序，点击切换为按名称排序'
                                        : '当前按名称排序，点击切换为按文章数降序'
                                }
                                className="inline-flex h-6 items-center gap-1 rounded-md border border-ui-line bg-white px-2 text-[11.5px] font-medium text-ui-sub transition-colors hover:border-ui-line-strong hover:text-ui-text"
                            >
                                <ArrowUpDown className="h-3 w-3" aria-hidden="true" />
                                {catSortMode === 'count' ? '按文章数' : '按名称'}
                            </button>
                        </div>

                        <div className="border-b border-ui-line/80 p-2.5">
                            <div className="relative">
                                <Search
                                    aria-hidden="true"
                                    className="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[#889B98]"
                                />
                                <label className="sr-only" htmlFor="category-directory-search">
                                    筛选分类
                                </label>
                                <input
                                    id="category-directory-search"
                                    type="text"
                                    value={catKeyword}
                                    onChange={(event) => setCatKeyword(event.target.value)}
                                    placeholder="筛选分类…"
                                    className="studio-input h-8 w-full pr-7 pl-8 text-[12.5px]"
                                />
                                {catKeyword ? (
                                    <button
                                        type="button"
                                        aria-label="清空分类筛选"
                                        onClick={() => setCatKeyword('')}
                                        className="absolute top-1/2 right-2 -translate-y-1/2 rounded p-0.5 text-ui-sub hover:bg-ui-bg hover:text-ui-text"
                                    >
                                        <X className="h-3.5 w-3.5" />
                                    </button>
                                ) : null}
                            </div>
                        </div>

                        {/* 置顶导航项：全部文章 + 未分类 */}
                        <div className="space-y-0.5 p-2">
                            <button
                                type="button"
                                aria-current={isAllCategoriesActive ? 'page' : undefined}
                                onClick={selectAllCategories}
                                className={cn(
                                    'flex w-full items-center justify-between gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors',
                                    isAllCategoriesActive
                                        ? 'bg-accent-tint font-medium text-accent-2'
                                        : 'text-ui-text hover:bg-ui-subtle',
                                )}
                            >
                                <span className="flex min-w-0 items-center gap-2">
                                    <Layers
                                        className={cn(
                                            'h-3.5 w-3.5 shrink-0',
                                            isAllCategoriesActive ? 'text-accent' : 'text-ui-sub',
                                        )}
                                        aria-hidden="true"
                                    />
                                    <span className="truncate">全部文章</span>
                                </span>
                                <span
                                    className={cn(
                                        'tnum shrink-0 rounded-full px-1.5 py-0.5 text-[11.5px]',
                                        isAllCategoriesActive
                                            ? 'bg-white/80 font-medium text-accent-2'
                                            : 'text-ui-sub',
                                    )}
                                >
                                    {totalCount ?? '—'}
                                </span>
                            </button>

                            <button
                                type="button"
                                aria-current={isUncategorizedActive ? 'page' : undefined}
                                onClick={selectUncategorized}
                                className={cn(
                                    'flex w-full items-center justify-between gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors',
                                    isUncategorizedActive
                                        ? 'bg-accent-tint font-medium text-accent-2'
                                        : 'text-ui-text hover:bg-ui-subtle',
                                )}
                            >
                                <span className="flex min-w-0 items-center gap-2">
                                    <Inbox
                                        className={cn(
                                            'h-3.5 w-3.5 shrink-0',
                                            isUncategorizedActive ? 'text-accent' : 'text-ui-sub',
                                        )}
                                        aria-hidden="true"
                                    />
                                    <span className="truncate">未分类</span>
                                </span>
                                <span
                                    className={cn(
                                        'tnum shrink-0 rounded-full px-1.5 py-0.5 text-[11.5px]',
                                        isUncategorizedActive
                                            ? 'bg-white/80 font-medium text-accent-2'
                                            : (uncategorizedCount ?? 0) > 0
                                              ? 'bg-warn-tint font-medium text-warn'
                                              : 'text-ui-sub',
                                    )}
                                >
                                    {uncategorizedCount ?? '—'}
                                </span>
                            </button>
                        </div>

                        <div className="mx-3 border-t border-ui-line" />

                        {/* 可滚动分类列表 */}
                        <div className="min-h-0 flex-1 overflow-y-auto p-2">
                            {categoryCountsQuery.isError && directoryHasNoData ? (
                                <div className="rounded-lg border border-[#F0C7C2] bg-bad-tint/70 p-3 text-[12px] text-bad">
                                    <div className="flex items-start gap-1.5">
                                        <AlertCircle
                                            className="mt-0.5 h-3.5 w-3.5 shrink-0"
                                            aria-hidden="true"
                                        />
                                        <span>分类统计加载失败</span>
                                    </div>
                                    <button
                                        type="button"
                                        onClick={() => void categoryCountsQuery.refetch()}
                                        className="mt-2 font-medium text-bad underline hover:opacity-80"
                                    >
                                        重试
                                    </button>
                                </div>
                            ) : categoryCountsQuery.isLoading && directoryHasNoData ? (
                                <div className="space-y-1.5 px-1 py-1" aria-busy="true">
                                    <div className="h-7 w-full animate-pulse rounded-lg bg-[#EFF4F2]" />
                                    <div className="h-7 w-5/6 animate-pulse rounded-lg bg-[#EFF4F2]" />
                                    <div className="h-7 w-4/5 animate-pulse rounded-lg bg-[#EFF4F2]" />
                                </div>
                            ) : directoryCategories.length === 0 ? (
                                <p className="px-2.5 py-4 text-center text-[12.5px] text-ui-sub">
                                    暂无分类
                                </p>
                            ) : filteredDirectoryCategories.length === 0 ? (
                                <div className="px-2.5 py-4 text-center text-[12.5px] text-ui-sub">
                                    <p>没有匹配的分类</p>
                                    <button
                                        type="button"
                                        onClick={() => setCatKeyword('')}
                                        className="mt-1.5 font-medium text-accent hover:underline"
                                    >
                                        清空关键词
                                    </button>
                                </div>
                            ) : (
                                <ul className="space-y-0.5">
                                    {filteredDirectoryCategories.map((cat) => {
                                        const isActive =
                                            !search.uncategorized && search.category_id === cat.id;
                                        return (
                                            <li key={cat.id}>
                                                <button
                                                    type="button"
                                                    aria-current={isActive ? 'page' : undefined}
                                                    onClick={() => selectCategory(cat.id)}
                                                    title={cat.name}
                                                    className={cn(
                                                        'flex w-full items-center justify-between gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors',
                                                        isActive
                                                            ? 'bg-accent-tint font-medium text-accent-2'
                                                            : 'text-ui-text hover:bg-ui-subtle',
                                                    )}
                                                >
                                                    <span className="truncate">{cat.name}</span>
                                                    <span
                                                        className={cn(
                                                            'tnum shrink-0 rounded-full px-1.5 py-0.5 text-[11.5px]',
                                                            isActive
                                                                ? 'bg-white/80 font-medium text-accent-2'
                                                                : 'text-ui-sub',
                                                        )}
                                                    >
                                                        {cat.post_count}
                                                    </span>
                                                </button>
                                            </li>
                                        );
                                    })}
                                </ul>
                            )}
                        </div>
                    </aside>
                ) : null}

                {/* 右侧主文章工作卡片 */}
                <div className="studio-card min-w-0 overflow-hidden">
                    {/* 当前分类上下文条 */}
                    <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ui-line bg-white px-4 py-3">
                        <div className="min-w-0">
                            <div className="flex flex-wrap items-center gap-2">
                                <FolderOpen
                                    className="h-4 w-4 shrink-0 text-accent"
                                    aria-hidden="true"
                                />
                                <h2 className="truncate text-[14.5px] font-semibold text-ui-text">
                                    {activeViewTitle}
                                </h2>
                                {activeViewCount !== undefined ? (
                                    <span className="tnum inline-flex items-center rounded-full bg-ui-subtle px-2 py-0.5 text-[12px] font-medium text-ui-sub">
                                        {activeViewCount} 篇
                                    </span>
                                ) : null}
                            </div>
                            {activeViewDescription ? (
                                <p className="mt-0.5 truncate text-[12.5px] text-ui-sub">
                                    {activeViewDescription}
                                </p>
                            ) : null}
                        </div>
                        {!isAllCategoriesActive ? (
                            <button
                                type="button"
                                onClick={selectAllCategories}
                                className="shrink-0 text-[12.5px] font-medium text-accent transition-colors hover:text-accent-2 hover:underline"
                            >
                                查看全部分类
                            </button>
                        ) : null}
                    </div>

                    {/* 移动端或桌面折叠态下的分类横向胶囊栏 */}
                    <nav
                        aria-label="分类快捷栏"
                        className={cn(
                            'border-b border-ui-line bg-ui-subtle/40 px-3.5 py-2.5',
                            !sidebarCollapsed && 'lg:hidden',
                        )}
                    >
                        <div className="flex items-center gap-1.5 overflow-x-auto pb-0.5">
                            <button
                                type="button"
                                aria-current={isAllCategoriesActive ? 'page' : undefined}
                                onClick={selectAllCategories}
                                className={cn(
                                    'inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-[12.5px] whitespace-nowrap transition-colors',
                                    isAllCategoriesActive
                                        ? 'border-accent/40 bg-accent-tint font-medium text-accent-2'
                                        : 'border-ui-line bg-white text-ui-sub hover:border-ui-line-strong hover:text-ui-text',
                                )}
                            >
                                <span>全部文章</span>
                                {totalCount !== undefined ? (
                                    <span className="tnum text-[11.5px] opacity-80">
                                        {totalCount}
                                    </span>
                                ) : null}
                            </button>

                            <button
                                type="button"
                                aria-current={isUncategorizedActive ? 'page' : undefined}
                                onClick={selectUncategorized}
                                className={cn(
                                    'inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-[12.5px] whitespace-nowrap transition-colors',
                                    isUncategorizedActive
                                        ? 'border-accent/40 bg-accent-tint font-medium text-accent-2'
                                        : (uncategorizedCount ?? 0) > 0
                                          ? 'border-warn/30 bg-warn-tint/70 text-warn hover:border-warn/50'
                                          : 'border-ui-line bg-white text-ui-sub hover:border-ui-line-strong hover:text-ui-text',
                                )}
                            >
                                <span>未分类</span>
                                {uncategorizedCount !== undefined ? (
                                    <span className="tnum text-[11.5px] opacity-85">
                                        {uncategorizedCount}
                                    </span>
                                ) : null}
                            </button>

                            {directoryCategories.length > 0 ? (
                                <span
                                    className="mx-0.5 h-4 w-px shrink-0 bg-ui-line"
                                    aria-hidden="true"
                                />
                            ) : null}

                            {directoryCategories.map((cat) => {
                                const isActive =
                                    !search.uncategorized && search.category_id === cat.id;
                                return (
                                    <button
                                        key={cat.id}
                                        type="button"
                                        aria-current={isActive ? 'page' : undefined}
                                        onClick={() => selectCategory(cat.id)}
                                        className={cn(
                                            'inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-[12.5px] whitespace-nowrap transition-colors',
                                            isActive
                                                ? 'border-accent/40 bg-accent-tint font-medium text-accent-2'
                                                : 'border-ui-line bg-white text-ui-sub hover:border-ui-line-strong hover:text-ui-text',
                                        )}
                                    >
                                        <span>{cat.name}</span>
                                        <span className="tnum text-[11.5px] opacity-75">
                                            {cat.post_count}
                                        </span>
                                    </button>
                                );
                            })}
                        </div>
                    </nav>

                    {/* 筛选栏 */}
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
                                        published:
                                            nextValue === '' ? undefined : nextValue === 'true',
                                    })
                                }
                                options={[
                                    { value: '', label: '全部线上状态' },
                                    { value: 'true', label: '已上线' },
                                    { value: 'false', label: '未上线' },
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

                            {canManageSeries ? (
                                <Select
                                    id="post-filter-series"
                                    ariaLabel="所属系列"
                                    value={
                                        search.series_id
                                            ? search.series_id
                                            : search.in_series === false
                                              ? '__none__'
                                              : search.in_series === true
                                                ? '__any__'
                                                : ''
                                    }
                                    onChange={(nextValue) => {
                                        if (nextValue === '') {
                                            updateSearch({
                                                series_id: undefined,
                                                in_series: undefined,
                                            });
                                        } else if (nextValue === '__none__') {
                                            updateSearch({
                                                series_id: undefined,
                                                in_series: false,
                                            });
                                        } else if (nextValue === '__any__') {
                                            updateSearch({
                                                series_id: undefined,
                                                in_series: true,
                                            });
                                        } else {
                                            updateSearch({
                                                series_id: nextValue,
                                                in_series: undefined,
                                            });
                                        }
                                    }}
                                    options={[
                                        { value: '', label: '全部系列' },
                                        { value: '__none__', label: '未归属系列' },
                                        { value: '__any__', label: '已归属系列' },
                                        ...(seriesListQuery.data?.data ?? []).map((item) => ({
                                            value: item.id,
                                            label: item.title,
                                        })),
                                    ]}
                                />
                            ) : null}

                            <Select
                                id="post-filter-sort"
                                ariaLabel="排序方式"
                                value={
                                    search.sort && search.sort !== 'updated_desc' ? search.sort : ''
                                }
                                onChange={(nextValue) =>
                                    updateSearch({
                                        sort: nextValue
                                            ? (nextValue as Schemas['PostSort'])
                                            : undefined,
                                    })
                                }
                                options={[
                                    { value: '', label: '最近更新' },
                                    { value: 'created_desc', label: '最近创建' },
                                    { value: 'title_asc', label: '标题 A→Z' },
                                    { value: 'title_desc', label: '标题 Z→A' },
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
                                            search={createLinkSearch}
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
                                <table className="tbl min-w-[820px]">
                                    <thead>
                                        <tr>
                                            <th className="w-[36%]">标题与 Slug</th>
                                            <th>发布状态</th>
                                            {canManageAll ? <th>作者</th> : null}
                                            <th>更新时间</th>
                                            <th className="text-right">操作</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {posts.map((post) => {
                                            const publication = postPublicationStatus(post);
                                            const visibleCategories = post.categories.slice(0, 3);
                                            const hiddenCategories = post.categories.slice(3);

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
                                                        <div className="mt-1.5 flex flex-wrap items-center gap-1">
                                                            {post.categories.length === 0 ? (
                                                                <button
                                                                    type="button"
                                                                    onClick={selectUncategorized}
                                                                    className="inline-flex items-center rounded-md border border-dashed border-ui-line px-1.5 py-0.5 text-[11px] text-ui-sub transition-colors hover:border-warn/50 hover:bg-warn-tint hover:text-warn"
                                                                >
                                                                    未分类
                                                                </button>
                                                            ) : (
                                                                <>
                                                                    {visibleCategories.map(
                                                                        (cat) => (
                                                                            <button
                                                                                key={cat.id}
                                                                                type="button"
                                                                                onClick={() =>
                                                                                    selectCategory(
                                                                                        cat.id,
                                                                                    )
                                                                                }
                                                                                className={cn(
                                                                                    'inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-[11.5px] font-medium transition-colors',
                                                                                    search.category_id ===
                                                                                        cat.id &&
                                                                                        !search.uncategorized
                                                                                        ? 'border-accent/40 bg-accent-tint text-accent-2'
                                                                                        : 'border-ui-line bg-ui-subtle text-ui-sub hover:border-accent/40 hover:bg-accent-tint hover:text-accent-2',
                                                                                )}
                                                                            >
                                                                                {cat.name}
                                                                            </button>
                                                                        ),
                                                                    )}
                                                                    {hiddenCategories.length > 0 ? (
                                                                        <span
                                                                            title={hiddenCategories
                                                                                .map((c) => c.name)
                                                                                .join('、')}
                                                                            className="tnum inline-flex items-center rounded-md border border-ui-line bg-white px-1.5 py-0.5 text-[11px] font-medium text-ui-sub"
                                                                        >
                                                                            +
                                                                            {
                                                                                hiddenCategories.length
                                                                            }
                                                                        </span>
                                                                    ) : null}
                                                                </>
                                                            )}
                                                            {canManageSeries && post.series ? (
                                                                <Link
                                                                    to="/series/$seriesId"
                                                                    params={{
                                                                        seriesId: post.series.id,
                                                                    }}
                                                                    title={
                                                                        post.series.section_title
                                                                            ? `${post.series.title} · ${post.series.section_title}`
                                                                            : post.series.title
                                                                    }
                                                                    className="inline-flex items-center gap-1 rounded-md border border-accent/30 bg-accent-tint/70 px-2 py-0.5 text-[11px] font-medium text-accent-2 transition-colors hover:border-accent hover:bg-accent-tint"
                                                                >
                                                                    <BookOpen className="h-3 w-3 shrink-0" />
                                                                    <span className="max-w-[140px] truncate">
                                                                        {post.series.title}
                                                                    </span>
                                                                    <span className="tnum font-code opacity-80">
                                                                        #{post.series.position + 1}
                                                                    </span>
                                                                </Link>
                                                            ) : null}
                                                        </div>
                                                    </td>
                                                    <td>
                                                        <StatusChip tone={publication.tone}>
                                                            {publication.label}
                                                        </StatusChip>
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

                            {pagination ? (
                                <PaginationBar
                                    page={pagination.page}
                                    perPage={pagination.per_page}
                                    total={pagination.total}
                                    totalPages={pagination.total_pages}
                                    unit="篇"
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
            </div>
            {deletion.dialog}
        </PageContainer>
    );
}
