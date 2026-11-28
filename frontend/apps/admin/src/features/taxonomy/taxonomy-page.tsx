import { zodResolver } from '@hookform/resolvers/zod';
import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import { Plus, Search } from 'lucide-react';
import { useMemo, useState } from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';
import { Button } from '@/components/ui/button';
import {
    EmptyState,
    ErrorBanner,
    LoadingCard,
    PageContainer,
    PageHeader,
} from '@/components/ui/feedback';
import { Drawer, Modal } from '@/components/ui/overlay';
import { useToast } from '@/components/ui/toast';
import { apiClient, unwrapResponse } from '@/lib/api';
import { applyApiFieldErrors, formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { queryKeys } from '@/lib/query-keys';

type TabKey = 'categories' | 'tags';

const SLUG_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

const termSchema = z.object({
    name: z.string().trim().min(1, '请输入名称').max(100, '名称最多 100 个字符'),
    slug: z
        .string()
        .trim()
        .min(1, '请输入 Slug')
        .max(100, 'Slug 最多 100 个字符')
        .regex(SLUG_PATTERN, '只能包含小写字母、数字和单个连字符'),
    description: z.string().max(500, '说明最多 500 个字符'),
});

type TermFormValues = z.infer<typeof termSchema>;

type DrawerTarget =
    | { mode: 'create'; kind: TabKey }
    | { mode: 'edit'; kind: 'categories'; item: Schemas['Category'] }
    | { mode: 'edit'; kind: 'tags'; item: Schemas['Tag'] }
    | null;

type DeleteTarget =
    | { kind: 'categories'; item: Schemas['Category'] }
    | { kind: 'tags'; item: Schemas['Tag'] }
    | null;

export function TaxonomyPage() {
    const queryClient = useQueryClient();
    const navigate = useNavigate({ from: '/taxonomy' });
    const search = useSearch({ from: '/_authed/taxonomy' });
    const { toast } = useToast();

    const activeTab: TabKey = search.tab === 'tags' ? 'tags' : 'categories';
    const [searchDraft, setSearchDraft] = useState<{ tab: TabKey; value: string }>({
        tab: activeTab,
        value: '',
    });
    const searchText = searchDraft.tab === activeTab ? searchDraft.value : '';
    const setSearchText = (value: string) => setSearchDraft({ tab: activeTab, value });

    const selectTab = (nextTab: TabKey) => {
        setSearchDraft({ tab: nextTab, value: '' });
        void navigate({
            to: '/taxonomy',
            search: { tab: nextTab },
        });
    };
    const [drawerTarget, setDrawerTarget] = useState<DrawerTarget>(null);
    const [deleteTarget, setDeleteTarget] = useState<DeleteTarget>(null);
    const [blockedDelete, setBlockedDelete] = useState<{
        name: string;
        count: number;
        kind: TabKey;
    } | null>(null);
    const [formTopError, setFormTopError] = useState<string | null>(null);

    const categoriesQuery = useQuery({
        queryKey: queryKeys.taxonomy.categories,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/categories')),
    });

    const tagsQuery = useQuery({
        queryKey: queryKeys.taxonomy.tags,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/tags')),
    });

    const {
        register,
        handleSubmit,
        reset,
        setError,
        formState: { errors },
    } = useForm<TermFormValues>({
        resolver: zodResolver(termSchema),
        defaultValues: { name: '', slug: '', description: '' },
    });

    const openDrawer = (target: NonNullable<DrawerTarget>) => {
        setFormTopError(null);
        setDrawerTarget(target);
        if (target.mode === 'create') {
            reset({ name: '', slug: '', description: '' });
        } else if (target.kind === 'categories') {
            reset({
                name: target.item.name,
                slug: target.item.slug,
                description: target.item.description ?? '',
            });
        } else {
            reset({
                name: target.item.name,
                slug: target.item.slug,
                description: '',
            });
        }
    };

    const saveMutation = useMutation({
        mutationFn: async (values: TermFormValues) => {
            if (!drawerTarget) return null;
            if (drawerTarget.mode === 'create' && drawerTarget.kind === 'categories') {
                await unwrapResponse(
                    apiClient.POST('/api/v1/admin/categories', {
                        body: {
                            name: values.name,
                            slug: values.slug,
                            description: values.description.trim() || null,
                        },
                    }),
                );
                return { kind: 'categories' as const, publication: null };
            }
            if (drawerTarget.mode === 'create' && drawerTarget.kind === 'tags') {
                await unwrapResponse(
                    apiClient.POST('/api/v1/admin/tags', {
                        body: { name: values.name, slug: values.slug },
                    }),
                );
                return { kind: 'tags' as const, publication: null };
            }
            if (drawerTarget.mode === 'edit' && drawerTarget.kind === 'categories') {
                const res = await unwrapResponse(
                    apiClient.PATCH('/api/v1/admin/categories/{id}', {
                        params: { path: { id: drawerTarget.item.id } },
                        body: {
                            name: values.name,
                            description: values.description.trim() || null,
                        },
                    }),
                );
                return { kind: 'categories' as const, publication: res.data.publication };
            }
            if (drawerTarget.mode === 'edit' && drawerTarget.kind === 'tags') {
                const res = await unwrapResponse(
                    apiClient.PATCH('/api/v1/admin/tags/{id}', {
                        params: { path: { id: drawerTarget.item.id } },
                        body: { name: values.name },
                    }),
                );
                return { kind: 'tags' as const, publication: res.data.publication };
            }
            return null;
        },
        onSuccess: (result) => {
            if (!result) return;
            void queryClient.invalidateQueries({
                queryKey:
                    result.kind === 'categories'
                        ? queryKeys.taxonomy.categories
                        : queryKeys.taxonomy.tags,
            });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            if (result.publication) {
                toast('公开内容已更新', 'ok');
                void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            } else {
                toast('已保存', 'ok');
            }
            setDrawerTarget(null);
        },
        onError: (error) => {
            const unmatched = applyApiFieldErrors(error, setError);
            if (unmatched.length > 0 || !isApiError(error) || error.code !== 'validation_error') {
                setFormTopError(formatErrorWithRequestId(error));
            }
        },
    });

    const deleteMutation = useMutation({
        mutationFn: async (target: NonNullable<DeleteTarget>) => {
            if (target.kind === 'categories') {
                await unwrapResponse(
                    apiClient.DELETE('/api/v1/admin/categories/{id}', {
                        params: { path: { id: target.item.id } },
                    }),
                );
            } else {
                await unwrapResponse(
                    apiClient.DELETE('/api/v1/admin/tags/{id}', {
                        params: { path: { id: target.item.id } },
                    }),
                );
            }
            return target;
        },
        onSuccess: (target) => {
            void queryClient.invalidateQueries({
                queryKey:
                    target.kind === 'categories'
                        ? queryKeys.taxonomy.categories
                        : queryKeys.taxonomy.tags,
            });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            setDeleteTarget(null);
            toast(`已删除「${target.item.name}」`, 'ok');
        },
        onError: (error, target) => {
            setDeleteTarget(null);
            if (isApiError(error) && error.code === 'taxonomy_in_use') {
                setBlockedDelete({
                    name: target.item.name,
                    count: Math.max(1, target.item.revision_count),
                    kind: target.kind,
                });
                return;
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const normalizedSearch = searchText.trim().toLowerCase();

    const filteredCategories = useMemo(() => {
        const list = categoriesQuery.data?.data ?? [];
        return list.filter(
            (item) =>
                !normalizedSearch ||
                item.name.toLowerCase().includes(normalizedSearch) ||
                item.slug.toLowerCase().includes(normalizedSearch),
        );
    }, [categoriesQuery.data?.data, normalizedSearch]);

    const filteredTags = useMemo(() => {
        const list = tagsQuery.data?.data ?? [];
        return list.filter(
            (item) =>
                !normalizedSearch ||
                item.name.toLowerCase().includes(normalizedSearch) ||
                item.slug.toLowerCase().includes(normalizedSearch),
        );
    }, [tagsQuery.data?.data, normalizedSearch]);

    const categories = categoriesQuery.data?.data ?? [];
    const tags = tagsQuery.data?.data ?? [];

    const handleRequestDelete = (target: NonNullable<DeleteTarget>) => {
        if (target.item.revision_count > 0) {
            setBlockedDelete({
                name: target.item.name,
                count: target.item.revision_count,
                kind: target.kind,
            });
            return;
        }
        setDeleteTarget(target);
    };

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="BLOG · TAXONOMY"
                title={activeTab === 'categories' ? '分类管理' : '标签管理'}
                description={
                    activeTab === 'categories'
                        ? '维护博客文章与专栏的分类体系；修改已用于公开内容的分类后，访客即可看到新名称。'
                        : '维护博客文章的标签体系；修改已用于公开内容的标签后，访客即可看到新名称。'
                }
                actions={
                    <Button
                        variant="primary"
                        onClick={() => openDrawer({ mode: 'create', kind: activeTab })}
                    >
                        <Plus className="h-4 w-4" />
                        {activeTab === 'categories' ? '新增分类' : '新增标签'}
                    </Button>
                }
            />

            <section className="studio-card overflow-hidden">
                <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ui-line bg-ui-subtle/60 px-4 py-3 sm:px-5">
                    <div
                        role="tablist"
                        aria-label="分类或标签"
                        className="inline-flex rounded-xl border border-ui-line bg-white p-1"
                    >
                        <button
                            type="button"
                            role="tab"
                            aria-selected={activeTab === 'categories'}
                            onClick={() => selectTab('categories')}
                            className="inline-flex items-center gap-1.5 rounded-lg px-3.5 py-1.5 text-[13px] font-medium text-ui-sub transition-colors aria-selected:bg-rail aria-selected:text-white"
                        >
                            <span>分类</span>
                            <span className="tnum text-[11.5px] opacity-75">
                                {categories.length}
                            </span>
                        </button>
                        <button
                            type="button"
                            role="tab"
                            aria-selected={activeTab === 'tags'}
                            onClick={() => selectTab('tags')}
                            className="inline-flex items-center gap-1.5 rounded-lg px-3.5 py-1.5 text-[13px] font-medium text-ui-sub transition-colors aria-selected:bg-rail aria-selected:text-white"
                        >
                            <span>标签</span>
                            <span className="tnum text-[11.5px] opacity-75">{tags.length}</span>
                        </button>
                    </div>

                    <div className="relative w-full sm:w-64">
                        <label htmlFor="taxonomy-search" className="sr-only">
                            搜索名称或 slug
                        </label>
                        <Search className="pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-ui-sub" />
                        <input
                            id="taxonomy-search"
                            type="search"
                            value={searchText}
                            onChange={(event) => setSearchText(event.target.value)}
                            placeholder="搜索名称或 slug"
                            className="studio-input pl-9"
                        />
                    </div>
                </div>

                {activeTab === 'categories' ? (
                    categoriesQuery.isError ? (
                        <div className="p-5">
                            <ErrorBanner
                                error={categoriesQuery.error}
                                onRetry={() => void categoriesQuery.refetch()}
                            />
                        </div>
                    ) : categoriesQuery.isLoading ? (
                        <div className="p-5">
                            <LoadingCard text="正在加载分类…" />
                        </div>
                    ) : filteredCategories.length === 0 ? (
                        <EmptyState
                            className="border-0 shadow-none"
                            title={normalizedSearch ? '没有匹配的分类' : '还没有分类'}
                            description="新增分类后，作者可以在文章设置里选择它。"
                            action={
                                <Button
                                    variant="primary"
                                    onClick={() =>
                                        openDrawer({ mode: 'create', kind: 'categories' })
                                    }
                                >
                                    <Plus className="h-4 w-4" />
                                    新增分类
                                </Button>
                            }
                        />
                    ) : (
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[700px]">
                                <thead>
                                    <tr>
                                        <th>名称</th>
                                        <th>Slug</th>
                                        <th>说明</th>
                                        <th className="text-right">文章数</th>
                                        <th className="text-right">引用修订数</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {filteredCategories.map((cat) => (
                                        <tr key={cat.id}>
                                            <td className="font-medium text-ui-text">{cat.name}</td>
                                            <td className="font-code text-[12.5px] text-ui-sub">
                                                {cat.slug}
                                            </td>
                                            <td className="max-w-[260px] truncate text-[13px] text-ui-sub">
                                                {cat.description || '—'}
                                            </td>
                                            <td className="tnum text-right text-[13px]">
                                                <Link
                                                    to="/posts"
                                                    search={{ category_id: cat.id }}
                                                    title={`查看「${cat.name}」下的文章`}
                                                    className="inline-flex items-center rounded-md px-2 py-0.5 font-medium text-accent transition-colors hover:bg-accent-tint hover:underline"
                                                >
                                                    {cat.post_count}
                                                </Link>
                                            </td>
                                            <td className="tnum text-right text-[13px]">
                                                {cat.revision_count}
                                            </td>
                                            <td className="whitespace-nowrap text-right">
                                                <Link
                                                    to="/posts"
                                                    search={{ category_id: cat.id }}
                                                    className="rounded-md px-2 py-1 text-[13px] text-ui-sub transition-colors hover:bg-ui-bg hover:text-ui-text"
                                                >
                                                    查看文章
                                                </Link>
                                                <button
                                                    type="button"
                                                    onClick={() =>
                                                        openDrawer({
                                                            mode: 'edit',
                                                            kind: 'categories',
                                                            item: cat,
                                                        })
                                                    }
                                                    className="rounded-md px-2 py-1 text-[13px] font-medium text-accent transition-colors hover:bg-accent-tint"
                                                >
                                                    编辑
                                                </button>
                                                <button
                                                    type="button"
                                                    onClick={() =>
                                                        handleRequestDelete({
                                                            kind: 'categories',
                                                            item: cat,
                                                        })
                                                    }
                                                    className="rounded-md px-2 py-1 text-[13px] font-medium text-bad transition-colors hover:bg-bad-tint"
                                                >
                                                    删除
                                                </button>
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </div>
                    )
                ) : tagsQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            error={tagsQuery.error}
                            onRetry={() => void tagsQuery.refetch()}
                        />
                    </div>
                ) : tagsQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载标签…" />
                    </div>
                ) : filteredTags.length === 0 ? (
                    <EmptyState
                        className="border-0 shadow-none"
                        title={normalizedSearch ? '没有匹配的标签' : '还没有标签'}
                        action={
                            <Button
                                variant="primary"
                                onClick={() => openDrawer({ mode: 'create', kind: 'tags' })}
                            >
                                <Plus className="h-4 w-4" />
                                新增标签
                            </Button>
                        }
                    />
                ) : (
                    <div className="overflow-x-auto">
                        <table className="tbl min-w-[660px]">
                            <thead>
                                <tr>
                                    <th>名称</th>
                                    <th>Slug</th>
                                    <th className="text-right">文章数</th>
                                    <th className="text-right">引用修订数</th>
                                    <th className="text-right">操作</th>
                                </tr>
                            </thead>
                            <tbody>
                                {filteredTags.map((tag) => (
                                    <tr key={tag.id}>
                                        <td className="font-medium text-ui-text">{tag.name}</td>
                                        <td className="font-code text-[12.5px] text-ui-sub">
                                            {tag.slug}
                                        </td>
                                        <td className="tnum text-right text-[13px]">
                                            <Link
                                                to="/posts"
                                                search={{ tag_id: tag.id }}
                                                title={`查看带有「${tag.name}」标签的文章`}
                                                className="inline-flex items-center rounded-md px-2 py-0.5 font-medium text-accent transition-colors hover:bg-accent-tint hover:underline"
                                            >
                                                {tag.post_count}
                                            </Link>
                                        </td>
                                        <td className="tnum text-right text-[13px]">
                                            {tag.revision_count}
                                        </td>
                                        <td className="whitespace-nowrap text-right">
                                            <Link
                                                to="/posts"
                                                search={{ tag_id: tag.id }}
                                                className="rounded-md px-2 py-1 text-[13px] text-ui-sub transition-colors hover:bg-ui-bg hover:text-ui-text"
                                            >
                                                查看文章
                                            </Link>
                                            <button
                                                type="button"
                                                onClick={() =>
                                                    openDrawer({
                                                        mode: 'edit',
                                                        kind: 'tags',
                                                        item: tag,
                                                    })
                                                }
                                                className="rounded-md px-2 py-1 text-[13px] font-medium text-accent transition-colors hover:bg-accent-tint"
                                            >
                                                编辑
                                            </button>
                                            <button
                                                type="button"
                                                onClick={() =>
                                                    handleRequestDelete({
                                                        kind: 'tags',
                                                        item: tag,
                                                    })
                                                }
                                                className="rounded-md px-2 py-1 text-[13px] font-medium text-bad transition-colors hover:bg-bad-tint"
                                            >
                                                删除
                                            </button>
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                )}
            </section>

            {/* 新增 / 编辑抽屉 */}
            <Drawer
                open={drawerTarget !== null}
                onClose={() => setDrawerTarget(null)}
                title={
                    drawerTarget
                        ? `${drawerTarget.mode === 'create' ? '新增' : '编辑'}${
                              drawerTarget.kind === 'categories' ? '分类' : '标签'
                          }`
                        : ''
                }
                footer={
                    <div className="flex justify-end gap-2">
                        <Button variant="secondary" onClick={() => setDrawerTarget(null)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            type="submit"
                            form="taxonomy-form"
                            disabled={saveMutation.isPending}
                        >
                            {saveMutation.isPending ? '保存中…' : '保存'}
                        </Button>
                    </div>
                }
            >
                <form
                    id="taxonomy-form"
                    className="space-y-4"
                    onSubmit={handleSubmit((values) => {
                        setFormTopError(null);
                        saveMutation.mutate(values);
                    })}
                    noValidate
                >
                    {formTopError ? (
                        <div
                            role="alert"
                            className="rounded-md border border-[#E8C3BF] bg-bad-tint px-3 py-2 text-[13px] text-bad"
                        >
                            {formTopError}
                        </div>
                    ) : null}

                    <div>
                        <label
                            htmlFor="term-name"
                            className="block text-[13px] font-medium text-ui-text"
                        >
                            名称
                        </label>
                        <input
                            id="term-name"
                            type="text"
                            className="studio-input mt-1.5"
                            {...register('name')}
                        />
                        {errors.name ? (
                            <p className="mt-1 text-[12px] text-bad">{errors.name.message}</p>
                        ) : null}
                    </div>

                    <div>
                        <label
                            htmlFor="term-slug"
                            className="block text-[13px] font-medium text-ui-text"
                        >
                            Slug
                        </label>
                        <input
                            id="term-slug"
                            type="text"
                            readOnly={drawerTarget?.mode === 'edit'}
                            className="studio-input mt-1.5 font-code text-[13px]"
                            {...register('slug')}
                        />
                        <p className="mt-1 text-[12px] text-ui-sub">
                            {drawerTarget?.mode === 'edit'
                                ? '创建后不可修改 Slug。'
                                : '只能包含小写字母、数字和连字符。'}
                        </p>
                        {errors.slug ? (
                            <p className="mt-1 text-[12px] text-bad">{errors.slug.message}</p>
                        ) : null}
                    </div>

                    {drawerTarget?.kind === 'categories' ? (
                        <div>
                            <label
                                htmlFor="term-desc"
                                className="block text-[13px] font-medium text-ui-text"
                            >
                                说明 <span className="font-normal text-ui-sub">（可选）</span>
                            </label>
                            <textarea
                                id="term-desc"
                                rows={3}
                                className="studio-input mt-1.5 h-auto py-2"
                                {...register('description')}
                            />
                            {errors.description ? (
                                <p className="mt-1 text-[12px] text-bad">
                                    {errors.description.message}
                                </p>
                            ) : null}
                        </div>
                    ) : null}

                    {drawerTarget?.mode === 'edit' ? (
                        <p className="rounded-md bg-ui-bg p-3 text-[13px] text-ui-sub">
                            若该{drawerTarget.kind === 'categories' ? '分类' : '标签'}
                            已用于公开文章，保存后访客即可看到新名称。
                        </p>
                    ) : null}
                </form>
            </Drawer>

            {/* 删除确认 */}
            <Modal
                open={deleteTarget !== null}
                onClose={() => setDeleteTarget(null)}
                role="alertdialog"
                title={deleteTarget ? `删除「${deleteTarget.item.name}」？` : ''}
                description="没有文章修订引用该条目。删除后无法恢复。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setDeleteTarget(null)}>
                            取消
                        </Button>
                        <Button
                            variant="danger"
                            disabled={deleteMutation.isPending}
                            onClick={() => {
                                if (deleteTarget) deleteMutation.mutate(deleteTarget);
                            }}
                        >
                            {deleteMutation.isPending ? '删除中…' : '确认删除'}
                        </Button>
                    </>
                }
            >
                <div className="font-code text-[13px] text-ui-sub">
                    slug: {deleteTarget?.item.slug}
                </div>
            </Modal>

            {/* 存在引用无法删除 */}
            <Modal
                open={blockedDelete !== null}
                onClose={() => setBlockedDelete(null)}
                role="alertdialog"
                title={blockedDelete ? `无法删除「${blockedDelete.name}」` : ''}
                footer={
                    <Button variant="primary" onClick={() => setBlockedDelete(null)}>
                        知道了
                    </Button>
                }
            >
                <p className="text-[14px] leading-6 text-ui-sub">
                    当前仍有{' '}
                    <span className="tnum font-medium text-ui-text">{blockedDelete?.count}</span>{' '}
                    条文章修订引用了这个{blockedDelete?.kind === 'categories' ? '分类' : '标签'}
                    （包括当前草稿和已发布文章）。请先解除相关文章的引用后再删除。
                </p>
            </Modal>
        </PageContainer>
    );
}
