import { postPublicationStatus } from '@/features/posts/post-publication-status';
import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import {
    AlertTriangle,
    ArrowDown,
    ArrowLeft,
    ArrowUp,
    BookOpen,
    ExternalLink,
    FilePlus2,
    FolderPlus,
    Globe,
    GripVertical,
    Plus,
    Save,
    Send,
    SlidersHorizontal,
    Trash2,
    X,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
    AddExistingPostsModal,
    QuickCreateSeriesPostModal,
    type OutlineTargetGroupOption,
} from './series-outline-modals';
import {
    canDeleteSeries,
    completionStatusMeta,
    isValidSeriesSlug,
    seriesPublishStatusMeta,
} from './series-status';
import { AuthenticatedImage } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import { ErrorBanner, LoadingCard, PageContainer } from '@/components/ui/feedback';
import { Drawer, Modal } from '@/components/ui/overlay';
import { Select } from '@/components/ui/select';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { MediaPickerDrawer } from '@/features/media/media-picker-drawer';
import { apiClient, unwrapResponse } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { formatDateTime } from '@/lib/format';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

const MAX_SECTIONS = 50;
const MAX_ITEMS = 200;

interface MetadataFormState {
    title: string;
    slug: string;
    description: string;
    cover_media_id: string | null;
    category_id: string | null;
    completion_status: 'ongoing' | 'completed';
    is_featured: boolean;
    sort_order: string;
    seo_title: string;
    seo_description: string;
}

interface OutlineDraftItem {
    post_id: string;
    title: string;
    slug: string;
    display_title: string;
    current_revision_id: string | null;
    published_revision_id: string | null;
}

interface OutlineDraftSection {
    localKey: string;
    id: string | null;
    title: string;
    description: string;
    items: OutlineDraftItem[];
}

interface OutlineDraft {
    items: OutlineDraftItem[];
    sections: OutlineDraftSection[];
}

interface DragPayload {
    groupKey: string;
    postId: string;
}

function seriesToMetadataForm(series: Schemas['Series']): MetadataFormState {
    return {
        title: series.title,
        slug: series.slug,
        description: series.description ?? '',
        cover_media_id: series.cover_media_id,
        category_id: series.category_id,
        completion_status: series.completion_status,
        is_featured: series.is_featured,
        sort_order: String(series.sort_order),
        seo_title: series.seo_title ?? '',
        seo_description: series.seo_description ?? '',
    };
}

function toDraftItem(item: Schemas['SeriesItem']): OutlineDraftItem {
    return {
        post_id: item.post_id,
        title: item.title,
        slug: item.slug,
        display_title: item.display_title ?? '',
        current_revision_id: item.current_revision_id,
        published_revision_id: item.published_revision_id,
    };
}

function seriesToOutlineDraft(series: Schemas['Series']): OutlineDraft {
    return {
        items: series.items.map(toDraftItem),
        sections: series.sections.map((section) => ({
            localKey: section.id,
            id: section.id,
            title: section.title,
            description: section.description ?? '',
            items: section.items.map(toDraftItem),
        })),
    };
}

function moveArrayItem<T>(list: T[], fromIndex: number, toIndex: number): T[] {
    if (fromIndex === toIndex || fromIndex < 0 || toIndex < 0 || toIndex >= list.length) {
        return list;
    }
    const next = [...list];
    const [moved] = next.splice(fromIndex, 1);
    if (moved !== undefined) next.splice(toIndex, 0, moved);
    return next;
}

export function SeriesWorkbenchPage({ seriesId }: { seriesId: string }) {
    const { user } = useAuth();
    const { toast } = useToast();
    const navigate = useNavigate();
    const queryClient = useQueryClient();

    const canPublish = hasPermission(user, PERMISSIONS.POSTS_PUBLISH);
    const canCreatePost = hasPermission(user, PERMISSIONS.POSTS_CREATE);

    const hydratedIdRef = useRef<string | null>(null);
    const newSectionSeqRef = useRef(1);

    const [metaForm, setMetaForm] = useState<MetadataFormState | null>(null);
    const [metaDirty, setMetaDirty] = useState(false);
    const [metaConflict, setMetaConflict] = useState(false);

    const [outline, setOutline] = useState<OutlineDraft>({ items: [], sections: [] });
    const [outlineDirty, setOutlineDirty] = useState(false);
    const [outlineConflict, setOutlineConflict] = useState<{
        code: string;
        message: string;
        latest: Schemas['Series'] | null;
    } | null>(null);

    const [dragPayload, setDragPayload] = useState<DragPayload | null>(null);
    const [expandedItemId, setExpandedItemId] = useState<string | null>(null);

    const [metaDrawerOpen, setMetaDrawerOpen] = useState(false);
    const [coverPickerOpen, setCoverPickerOpen] = useState(false);
    const closeMetaDrawer = useCallback(() => {
        if (coverPickerOpen) return;
        setMetaDrawerOpen(false);
    }, [coverPickerOpen]);
    const [addExistingOpen, setAddExistingOpen] = useState(false);
    const [quickCreateOpen, setQuickCreateOpen] = useState(false);
    const [publishModalOpen, setPublishModalOpen] = useState(false);
    const [unpublishModalOpen, setUnpublishModalOpen] = useState(false);
    const [deleteModalOpen, setDeleteModalOpen] = useState(false);
    const [deleteSectionTarget, setDeleteSectionTarget] = useState<OutlineDraftSection | null>(
        null,
    );
    const [publishingPostIds, setPublishingPostIds] = useState<Set<string>>(() => new Set());
    const [unpublishPostTarget, setUnpublishPostTarget] = useState<OutlineDraftItem | null>(null);
    const [batchPublishModalOpen, setBatchPublishModalOpen] = useState(false);

    const seriesQuery = useQuery({
        queryKey: queryKeys.series.detail(seriesId),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/series/{id}', {
                    params: { path: { id: seriesId } },
                }),
            );
            return res.data;
        },
    });

    const categoriesQuery = useQuery({
        queryKey: queryKeys.taxonomy.categories,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/categories')),
    });

    // 首次加载或切换 seriesId 时初始化本地状态
    useEffect(() => {
        const data = seriesQuery.data;
        if (!data) return;
        if (hydratedIdRef.current !== data.id) {
            hydratedIdRef.current = data.id;
            setMetaForm(seriesToMetadataForm(data));
            setMetaDirty(false);
            setMetaConflict(false);
            setOutline(seriesToOutlineDraft(data));
            setOutlineDirty(false);
            setOutlineConflict(null);
        }
    }, [seriesQuery.data]);

    const series = seriesQuery.data;
    const categories = categoriesQuery.data?.data ?? [];

    const totalOutlineItems = useMemo(
        () =>
            outline.items.length +
            outline.sections.reduce((sum, section) => sum + section.items.length, 0),
        [outline],
    );

    const existingPostIds = useMemo(() => {
        const ids = new Set<string>();
        for (const item of outline.items) ids.add(item.post_id);
        for (const section of outline.sections) {
            for (const item of section.items) ids.add(item.post_id);
        }
        return ids;
    }, [outline]);

    const groupOptions = useMemo<OutlineTargetGroupOption[]>(
        () => [
            { key: '__root__', serverId: null, label: '根目录（系列顶部）' },
            ...outline.sections.map((section, idx) => ({
                key: section.localKey,
                serverId: section.id,
                label: `篇章 ${idx + 1}：${section.title.trim() || '未命名篇章'}`,
            })),
        ],
        [outline.sections],
    );

    const syncAfterSeriesWrite = useCallback(
        (updated: Schemas['Series'], syncMeta: boolean, syncOutline: boolean) => {
            queryClient.setQueryData(queryKeys.series.detail(seriesId), updated);
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            if (syncMeta) {
                setMetaForm(seriesToMetadataForm(updated));
                setMetaDirty(false);
                setMetaConflict(false);
            }
            if (syncOutline) {
                setOutline(seriesToOutlineDraft(updated));
                setOutlineDirty(false);
                setOutlineConflict(null);
            }
        },
        [queryClient, seriesId],
    );

    // 1. 保存基础信息
    const saveMetaMutation = useMutation({
        mutationFn: async (input: { form: MetadataFormState; current: Schemas['Series'] }) => {
            const { form, current } = input;
            const patch: Schemas['UpdateSeriesInput'] = {
                expected_edit_version: current.edit_version,
            };
            const nextTitle = form.title.trim();
            if (nextTitle !== current.title) patch.title = nextTitle;
            const nextSlug = form.slug.trim();
            if (!current.slug_locked && nextSlug !== current.slug) patch.slug = nextSlug;
            const nextDesc = form.description.trim() || null;
            if (nextDesc !== current.description) patch.description = nextDesc;
            if (form.cover_media_id !== current.cover_media_id) {
                patch.cover_media_id = form.cover_media_id;
            }
            if (form.category_id !== current.category_id) {
                patch.category_id = form.category_id;
            }
            if (form.completion_status !== current.completion_status) {
                patch.completion_status = form.completion_status;
            }
            if (form.is_featured !== current.is_featured) {
                patch.is_featured = form.is_featured;
            }
            const nextSort = Number.parseInt(form.sort_order.trim() || '0', 10);
            const normalizedSort = Number.isFinite(nextSort) ? nextSort : 0;
            if (normalizedSort !== current.sort_order) patch.sort_order = normalizedSort;
            const nextSeoTitle = form.seo_title.trim() || null;
            if (nextSeoTitle !== current.seo_title) patch.seo_title = nextSeoTitle;
            const nextSeoDesc = form.seo_description.trim() || null;
            if (nextSeoDesc !== current.seo_description) patch.seo_description = nextSeoDesc;

            const res = await unwrapResponse(
                apiClient.PATCH('/api/v1/admin/series/{id}', {
                    params: { path: { id: seriesId } },
                    body: patch,
                }),
            );
            return res.data;
        },
        onSuccess: (edited) => {
            syncAfterSeriesWrite(edited.series, true, !outlineDirty);
            if (edited.publication)
                void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            setMetaDrawerOpen(false);
            toast(edited.publication ? '公开基础信息已更新' : '基础信息已保存', 'ok');
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'series_edit_conflict') {
                setMetaConflict(true);
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const handleSaveMetadata = () => {
        if (!metaForm || !series) return;
        if (!metaForm.title.trim() || metaForm.title.trim().length > 100) {
            toast('系列标题需为 1–100 个字符', 'warn');
            return;
        }
        if (!series.slug_locked && !isValidSeriesSlug(metaForm.slug)) {
            toast('Slug 格式无效（仅允许小写字母、数字和单个连字符）', 'warn');
            return;
        }
        saveMetaMutation.mutate({ form: metaForm, current: series });
    };

    // 2. 保存完整目录
    const buildOutlinePayload = useCallback(
        (draft: OutlineDraft, expectedEditVersion: number): Schemas['SeriesOutlineInput'] => ({
            expected_edit_version: expectedEditVersion,
            items: draft.items.map((item) => ({
                post_id: item.post_id,
                display_title: item.display_title.trim() || null,
            })),
            sections: draft.sections.map((section) => ({
                id: section.id,
                title: section.title.trim(),
                description: section.description.trim() || null,
                items: section.items.map((item) => ({
                    post_id: item.post_id,
                    display_title: item.display_title.trim() || null,
                })),
            })),
        }),
        [],
    );

    const validateOutlineBeforeSave = (draft: OutlineDraft): string | null => {
        if (draft.sections.length > MAX_SECTIONS) {
            return `单系列最多允许 ${MAX_SECTIONS} 个篇章分组。`;
        }
        const total =
            draft.items.length + draft.sections.reduce((acc, s) => acc + s.items.length, 0);
        if (total > MAX_ITEMS) {
            return `单系列最多允许编排 ${MAX_ITEMS} 篇文章。`;
        }
        for (let i = 0; i < draft.sections.length; i += 1) {
            const sec = draft.sections[i]!;
            const trimmedTitle = sec.title.trim();
            if (!trimmedTitle || trimmedTitle.length > 100) {
                return `第 ${i + 1} 个篇章标题需为 1–100 个字符。`;
            }
            if (sec.description.trim().length > 500) {
                return `篇章「${trimmedTitle}」简介不能超过 500 个字符。`;
            }
        }
        return null;
    };

    const saveOutlineMutation = useMutation({
        mutationFn: async (input: { draft: OutlineDraft; expectedEditVersion: number }) => {
            const res = await unwrapResponse(
                apiClient.PUT('/api/v1/admin/series/{id}/outline', {
                    params: { path: { id: seriesId } },
                    body: buildOutlinePayload(input.draft, input.expectedEditVersion),
                }),
            );
            return res.data;
        },
        onSuccess: (edited) => {
            syncAfterSeriesWrite(edited.series, !metaDirty, true);
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            if (edited.publication)
                void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            toast(edited.publication ? '公开目录已更新' : '目录已保存', 'ok');
        },
        onError: async (error) => {
            if (
                isApiError(error) &&
                (error.code === 'series_edit_conflict' || error.code === 'post_already_in_series')
            ) {
                let latest: Schemas['Series'] | null = null;
                try {
                    const refreshed = await unwrapResponse(
                        apiClient.GET('/api/v1/admin/series/{id}', {
                            params: { path: { id: seriesId } },
                        }),
                    );
                    latest = refreshed.data;
                    queryClient.setQueryData(queryKeys.series.detail(seriesId), latest);
                } catch {
                    // keep latest as null
                }
                setOutlineConflict({
                    code: error.code,
                    message: formatErrorWithRequestId(error),
                    latest,
                });
                return;
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const handleSaveOutline = (overrideVersion?: number) => {
        if (!series) return;
        const validationError = validateOutlineBeforeSave(outline);
        if (validationError) {
            toast(validationError, 'warn');
            return;
        }
        saveOutlineMutation.mutate({
            draft: outline,
            expectedEditVersion: overrideVersion ?? series.edit_version,
        });
    };

    // 3. 公开 / 取消公开 / 删除系列
    const publishMutation = useMutation({
        mutationFn: async (expectedPubVersion: number) => {
            const res = await unwrapResponse(
                apiClient.PUT('/api/v1/admin/series/{id}/publication', {
                    params: { path: { id: seriesId } },
                    body: { expected_publication_version: expectedPubVersion },
                }),
            );
            return res.data;
        },
        onSuccess: () => {
            setPublishModalOpen(false);
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            toast('系列公开设置已保存', 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'publication_conflict') {
                void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const unpublishMutation = useMutation({
        mutationFn: async (expectedPubVersion: number) => {
            const res = await unwrapResponse(
                apiClient.DELETE('/api/v1/admin/series/{id}/publication', {
                    params: {
                        path: { id: seriesId },
                        query: { expected_publication_version: expectedPubVersion },
                    },
                }),
            );
            return res.data;
        },
        onSuccess: () => {
            setUnpublishModalOpen(false);
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            toast('系列已取消公开', 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'publication_conflict') {
                void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const deleteMutation = useMutation({
        mutationFn: async (current: Schemas['Series']) => {
            await unwrapResponse(
                apiClient.DELETE('/api/v1/admin/series/{id}', {
                    params: {
                        path: { id: seriesId },
                        query: {
                            expected_edit_version: current.edit_version,
                            expected_publication_version: current.publication_version,
                        },
                    },
                }),
            );
        },
        onSuccess: () => {
            setDeleteModalOpen(false);
            toast('系列已删除（文章与修订均已保留）', 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            void navigate({ to: '/series' });
        },
        onError: (error) => {
            setDeleteModalOpen(false);
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    // 4. 在工作台内直接发布 / 下线单篇文章，以及批量发布未发布文章
    const patchOutlineItemStatus = useCallback(
        (
            postId: string,
            patch: Partial<Pick<OutlineDraftItem, 'current_revision_id' | 'published_revision_id'>>,
        ) => {
            setOutline((prev) => ({
                items: prev.items.map((it) => (it.post_id === postId ? { ...it, ...patch } : it)),
                sections: prev.sections.map((sec) => ({
                    ...sec,
                    items: sec.items.map((it) =>
                        it.post_id === postId ? { ...it, ...patch } : it,
                    ),
                })),
            }));
        },
        [],
    );

    const publishSinglePostRequest = async (postId: string) => {
        const postRes = await unwrapResponse(
            apiClient.GET('/api/v1/admin/posts/{id}', {
                params: { path: { id: postId } },
            }),
        );
        const postDetail = postRes.data;
        await unwrapResponse(
            apiClient.PUT('/api/v1/admin/posts/{id}/publication', {
                params: { path: { id: postId } },
                body: {
                    revision_id: postDetail.current_revision_id,
                    expected_publication_version: postDetail.publication.publication_version,
                },
            }),
        );
        return postDetail.current_revision_id;
    };

    const publishPostMutation = useMutation({
        mutationFn: async (item: OutlineDraftItem) => {
            const revisionId = await publishSinglePostRequest(item.post_id);
            return { item, revisionId };
        },
        onMutate: (item) => {
            setPublishingPostIds((prev) => {
                const next = new Set(prev);
                next.add(item.post_id);
                return next;
            });
        },
        onSuccess: ({ item, revisionId }) => {
            patchOutlineItemStatus(item.post_id, {
                current_revision_id: revisionId,
                published_revision_id: revisionId,
            });
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            toast(`已发布「${item.title}」`, 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(item.post_id) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
        },
        onError: (error, item) => {
            toast(`发布「${item.title}」失败：${formatErrorWithRequestId(error)}`, 'bad');
        },
        onSettled: (_data, _error, item) => {
            setPublishingPostIds((prev) => {
                const next = new Set(prev);
                next.delete(item.post_id);
                return next;
            });
        },
    });

    const unpublishPostMutation = useMutation({
        mutationFn: async (item: OutlineDraftItem) => {
            const postRes = await unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}', {
                    params: { path: { id: item.post_id } },
                }),
            );
            await unwrapResponse(
                apiClient.DELETE('/api/v1/admin/posts/{id}/publication', {
                    params: {
                        path: { id: item.post_id },
                        query: {
                            expected_publication_version:
                                postRes.data.publication.publication_version,
                        },
                    },
                }),
            );
            return item;
        },
        onSuccess: (item) => {
            setUnpublishPostTarget(null);
            patchOutlineItemStatus(item.post_id, { published_revision_id: null });
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            toast(`「${item.title}」已下线`, 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(item.post_id) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
        },
        onError: (error, item) => {
            toast(`取消发布「${item.title}」失败：${formatErrorWithRequestId(error)}`, 'bad');
        },
    });

    const batchPublishMutation = useMutation({
        mutationFn: async (targets: OutlineDraftItem[]) => {
            let succeeded = 0;
            const failed: Array<{ title: string; reason: string }> = [];

            for (const item of targets) {
                try {
                    const revisionId = await publishSinglePostRequest(item.post_id);
                    succeeded += 1;
                    patchOutlineItemStatus(item.post_id, {
                        current_revision_id: revisionId,
                        published_revision_id: revisionId,
                    });
                } catch (error) {
                    failed.push({
                        title: item.title,
                        reason: formatErrorWithRequestId(error),
                    });
                }
            }
            return { succeeded, failed };
        },
        onSuccess: ({ succeeded, failed }) => {
            setBatchPublishModalOpen(false);
            if (succeeded > 0) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
                void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(seriesId) });
                void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            }
            if (failed.length === 0) {
                toast(`已发布 ${succeeded} 篇文章`, 'ok');
            } else if (succeeded > 0) {
                toast(
                    `已发布 ${succeeded} 篇，${failed.length} 篇失败（首项：${failed[0]!.title}）`,
                    'warn',
                );
            } else {
                toast(`批量发布失败：${failed[0]?.reason ?? '请稍后重试'}`, 'bad');
            }
        },
    });

    // 本地目录编排辅助函数
    const patchOutline = (updater: (prev: OutlineDraft) => OutlineDraft) => {
        setOutline((prev) => updater(prev));
        setOutlineDirty(true);
    };

    const handleAddSection = () => {
        if (outline.sections.length >= MAX_SECTIONS) {
            toast(`单系列最多创建 ${MAX_SECTIONS} 个篇章分组`, 'warn');
            return;
        }
        const seq = newSectionSeqRef.current++;
        patchOutline((prev) => ({
            ...prev,
            sections: [
                ...prev.sections,
                {
                    localKey: `new-section-${seq}`,
                    id: null,
                    title: `新篇章 ${prev.sections.length + 1}`,
                    description: '',
                    items: [],
                },
            ],
        }));
    };

    const handleMoveSection = (index: number, delta: -1 | 1) => {
        patchOutline((prev) => ({
            ...prev,
            sections: moveArrayItem(prev.sections, index, index + delta),
        }));
    };

    const handleRemoveSection = (section: OutlineDraftSection, moveItemsToRoot: boolean) => {
        patchOutline((prev) => ({
            items: moveItemsToRoot ? [...prev.items, ...section.items] : prev.items,
            sections: prev.sections.filter((s) => s.localKey !== section.localKey),
        }));
        setDeleteSectionTarget(null);
    };

    const handleAddExistingPosts = (posts: Schemas['PostSummary'][], targetGroupKey: string) => {
        const remaining = MAX_ITEMS - totalOutlineItems;
        if (remaining <= 0) {
            toast(`单系列最多包含 ${MAX_ITEMS} 篇文章`, 'warn');
            return;
        }
        const slice = posts.slice(0, remaining);
        if (slice.length < posts.length) {
            toast(`超出 ${MAX_ITEMS} 篇上限，仅加入了前 ${slice.length} 篇`, 'warn');
        }
        const newItems: OutlineDraftItem[] = slice.map((post) => ({
            post_id: post.id,
            title: post.title,
            slug: post.slug,
            display_title: '',
            current_revision_id: post.current_revision_id,
            published_revision_id: post.published_revision_id,
        }));
        patchOutline((prev) => {
            if (targetGroupKey === '__root__') {
                return { ...prev, items: [...prev.items, ...newItems] };
            }
            return {
                ...prev,
                sections: prev.sections.map((section) =>
                    section.localKey === targetGroupKey
                        ? { ...section, items: [...section.items, ...newItems] }
                        : section,
                ),
            };
        });
    };

    const handleMoveItemWithinGroup = (groupKey: string, index: number, delta: -1 | 1) => {
        patchOutline((prev) => {
            if (groupKey === '__root__') {
                return { ...prev, items: moveArrayItem(prev.items, index, index + delta) };
            }
            return {
                ...prev,
                sections: prev.sections.map((sec) =>
                    sec.localKey === groupKey
                        ? { ...sec, items: moveArrayItem(sec.items, index, index + delta) }
                        : sec,
                ),
            };
        });
    };

    const handleMoveItemAcrossGroups = (
        fromGroupKey: string,
        postId: string,
        toGroupKey: string,
        toIndex?: number,
    ) => {
        if (fromGroupKey === toGroupKey && toIndex === undefined) return;
        patchOutline((prev) => {
            let moving: OutlineDraftItem | undefined;
            const removeFrom = (list: OutlineDraftItem[]) => {
                const idx = list.findIndex((item) => item.post_id === postId);
                if (idx === -1) return list;
                moving = list[idx];
                return [...list.slice(0, idx), ...list.slice(idx + 1)];
            };
            const nextRoot = fromGroupKey === '__root__' ? removeFrom(prev.items) : [...prev.items];
            const nextSections = prev.sections.map((sec) =>
                sec.localKey === fromGroupKey
                    ? { ...sec, items: removeFrom(sec.items) }
                    : { ...sec, items: [...sec.items] },
            );
            if (!moving) return prev;

            const insertInto = (list: OutlineDraftItem[]) => {
                const copy = [...list];
                const idx =
                    toIndex !== undefined && toIndex >= 0 && toIndex <= copy.length
                        ? toIndex
                        : copy.length;
                copy.splice(idx, 0, moving!);
                return copy;
            };

            if (toGroupKey === '__root__') {
                return { items: insertInto(nextRoot), sections: nextSections };
            }
            return {
                items: nextRoot,
                sections: nextSections.map((sec) =>
                    sec.localKey === toGroupKey ? { ...sec, items: insertInto(sec.items) } : sec,
                ),
            };
        });
    };

    const handleRemoveItem = (groupKey: string, postId: string) => {
        patchOutline((prev) => {
            if (groupKey === '__root__') {
                return { ...prev, items: prev.items.filter((i) => i.post_id !== postId) };
            }
            return {
                ...prev,
                sections: prev.sections.map((sec) =>
                    sec.localKey === groupKey
                        ? { ...sec, items: sec.items.filter((i) => i.post_id !== postId) }
                        : sec,
                ),
            };
        });
    };

    const handleUpdateDisplayTitle = (groupKey: string, postId: string, value: string) => {
        patchOutline((prev) => {
            const updateList = (list: OutlineDraftItem[]) =>
                list.map((i) => (i.post_id === postId ? { ...i, display_title: value } : i));
            if (groupKey === '__root__') {
                return { ...prev, items: updateList(prev.items) };
            }
            return {
                ...prev,
                sections: prev.sections.map((sec) =>
                    sec.localKey === groupKey ? { ...sec, items: updateList(sec.items) } : sec,
                ),
            };
        });
    };

    const handleSaveOutlineForQuickCreate = async () => {
        if (!series) throw new Error('系列数据尚未加载');
        const validationError = validateOutlineBeforeSave(outline);
        if (validationError) throw new Error(validationError);
        const res = await unwrapResponse(
            apiClient.PUT('/api/v1/admin/series/{id}/outline', {
                params: { path: { id: seriesId } },
                body: buildOutlinePayload(outline, series.edit_version),
            }),
        );
        const savedSeries = res.data.series;
        const map = new Map<string, string | null>();
        map.set('__root__', null);
        // 服务端按提交顺序返回 sections，可按索引精确映射每个本地 localKey 到真实 UUID
        outline.sections.forEach((localSec, idx) => {
            const serverSec = savedSeries.sections[idx];
            if (serverSec) map.set(localSec.localKey, serverSec.id);
        });
        syncAfterSeriesWrite(savedSeries, !metaDirty, true);
        if (res.data.publication)
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
        return { series: savedSeries, groupKeyToSectionId: map };
    };

    const handleRefreshSeries = async (): Promise<Schemas['Series']> => {
        const res = await unwrapResponse(
            apiClient.GET('/api/v1/admin/series/{id}', {
                params: { path: { id: seriesId } },
            }),
        );
        queryClient.setQueryData(queryKeys.series.detail(seriesId), res.data);
        if (!outlineDirty) {
            setOutline(seriesToOutlineDraft(res.data));
        }
        if (!metaDirty) {
            setMetaForm(seriesToMetadataForm(res.data));
        }
        return res.data;
    };

    if (seriesQuery.isLoading || !metaForm) {
        return (
            <PageContainer>
                <LoadingCard text="正在加载系列工作台…" />
            </PageContainer>
        );
    }

    if (seriesQuery.isError || !series) {
        return (
            <PageContainer>
                <ErrorBanner
                    title="无法读取系列详情"
                    error={seriesQuery.error}
                    onRetry={() => void seriesQuery.refetch()}
                />
            </PageContainer>
        );
    }

    const pubStatus = seriesPublishStatusMeta(series);
    const compStatus = completionStatusMeta(series.completion_status);
    const deletable = canDeleteSeries(series);

    // 服务端最新文章发布状态映射（刷新后无需覆盖本地目录编排即可实时同步状态徽章）
    const serverPostStatusById = new Map<
        string,
        Pick<OutlineDraftItem, 'current_revision_id' | 'published_revision_id'>
    >();
    for (const it of series.items) {
        serverPostStatusById.set(it.post_id, {
            current_revision_id: it.current_revision_id,
            published_revision_id: it.published_revision_id,
        });
    }
    for (const sec of series.sections) {
        for (const it of sec.items) {
            serverPostStatusById.set(it.post_id, {
                current_revision_id: it.current_revision_id,
                published_revision_id: it.published_revision_id,
            });
        }
    }

    const resolveItemStatus = (item: OutlineDraftItem) => {
        const server = serverPostStatusById.get(item.post_id);
        return {
            current_revision_id: server?.current_revision_id ?? item.current_revision_id,
            published_revision_id: server
                ? server.published_revision_id
                : item.published_revision_id,
        };
    };

    const allOutlineItems: OutlineDraftItem[] = [
        ...outline.items,
        ...outline.sections.flatMap((sec) => sec.items),
    ];
    const publishableItems = allOutlineItems.filter((item) => {
        const st = resolveItemStatus(item);
        return (
            !st.published_revision_id ||
            (Boolean(st.current_revision_id) && st.current_revision_id !== st.published_revision_id)
        );
    });

    // 计算每一项在全系列中的 1-based 预览序号（纯函数式计算，无 render 期间变量重赋值）
    const rootWithOrder = outline.items.map((item, idx) => ({ item, order: idx + 1 }));
    const sectionsWithOrder = outline.sections.map((section, sectionIdx) => {
        const previousCount =
            outline.items.length +
            outline.sections.slice(0, sectionIdx).reduce((sum, prev) => sum + prev.items.length, 0);
        return {
            section,
            items: section.items.map((item, idx) => ({ item, order: previousCount + idx + 1 })),
        };
    });

    const renderItemRow = (
        groupKey: string,
        item: OutlineDraftItem,
        indexInGroup: number,
        groupLength: number,
        globalOrder: number,
    ) => {
        const effectiveStatus = resolveItemStatus(item);
        const postChip = postPublicationStatus(effectiveStatus);
        const isExpanded = expandedItemId === item.post_id;
        const hasDesiredPub = Boolean(effectiveStatus.published_revision_id);
        const needsPublish =
            !hasDesiredPub ||
            (Boolean(effectiveStatus.current_revision_id) &&
                effectiveStatus.current_revision_id !== effectiveStatus.published_revision_id);
        const isPublishingThis = publishingPostIds.has(item.post_id);
        const isUnpublishingThis =
            unpublishPostMutation.isPending && unpublishPostTarget?.post_id === item.post_id;

        return (
            <li key={item.post_id}>
                <div
                    onDragOver={(event) => event.preventDefault()}
                    onDrop={(event) => {
                        event.preventDefault();
                        if (!dragPayload || dragPayload.postId === item.post_id) return;
                        handleMoveItemAcrossGroups(
                            dragPayload.groupKey,
                            dragPayload.postId,
                            groupKey,
                            indexInGroup,
                        );
                        setDragPayload(null);
                    }}
                    className={cn(
                        'flex flex-col gap-2.5 rounded-xl border border-ui-line bg-white px-3 py-2.5 transition-colors',
                        dragPayload?.postId === item.post_id && 'opacity-50',
                    )}
                >
                    <div className="flex items-start gap-2.5">
                        <button
                            type="button"
                            draggable
                            onDragStart={() => setDragPayload({ groupKey, postId: item.post_id })}
                            onDragEnd={() => setDragPayload(null)}
                            title="拖拽调整顺序"
                            aria-label={`拖拽调整 ${item.title} 的顺序`}
                            className="mt-1 cursor-grab rounded p-0.5 text-[#9AA8A3] hover:bg-ui-subtle hover:text-ui-text active:cursor-grabbing"
                        >
                            <GripVertical className="h-4 w-4" aria-hidden="true" />
                        </button>
                        <span className="tnum mt-0.5 inline-flex h-6 min-w-7 shrink-0 items-center justify-center rounded-md bg-ui-subtle px-1.5 font-code text-[11.5px] font-semibold text-ui-sub">
                            {String(globalOrder).padStart(2, '0')}
                        </span>

                        <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-2">
                                <Link
                                    to="/posts/$postId/edit"
                                    params={{ postId: item.post_id }}
                                    className="inline-flex items-center gap-1 font-medium text-ui-text transition-colors hover:text-accent"
                                >
                                    <span className="truncate">{item.title}</span>
                                    <ExternalLink className="h-3 w-3 shrink-0 opacity-60" />
                                </Link>
                                <StatusChip tone={postChip.tone}>{postChip.label}</StatusChip>
                            </div>
                            <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-[11.5px] text-ui-sub">
                                <span className="font-code">/blog/{item.slug}/</span>
                                {item.display_title.trim() ? (
                                    <span className="truncate">
                                        · 目录短标题：{item.display_title.trim()}
                                    </span>
                                ) : null}
                            </div>
                        </div>

                        <div className="flex shrink-0 items-center gap-1">
                            {canPublish && needsPublish ? (
                                <button
                                    type="button"
                                    disabled={isPublishingThis || batchPublishMutation.isPending}
                                    onClick={() => publishPostMutation.mutate(item)}
                                    title={hasDesiredPub ? '发布最新保存的修订' : '立即发布该文章'}
                                    aria-label={`${hasDesiredPub ? '发布更新' : '发布'} ${item.title}`}
                                    className="mr-0.5 inline-flex h-7 items-center gap-1 rounded-md bg-accent-tint px-2.5 text-[12px] font-medium text-accent-2 transition-colors hover:bg-accent hover:text-white disabled:opacity-50"
                                >
                                    <Send className="h-3 w-3" />
                                    {isPublishingThis
                                        ? '发布中…'
                                        : hasDesiredPub
                                          ? '发布更新'
                                          : '发布'}
                                </button>
                            ) : null}
                            {canPublish && hasDesiredPub ? (
                                <button
                                    type="button"
                                    disabled={isUnpublishingThis || batchPublishMutation.isPending}
                                    onClick={() => setUnpublishPostTarget(item)}
                                    title="取消发布该文章（从公开站点下线）"
                                    aria-label={`取消发布 ${item.title}`}
                                    className="mr-0.5 inline-flex h-7 items-center rounded-md px-2 text-[12px] font-medium text-ui-sub transition-colors hover:bg-bad-tint hover:text-bad disabled:opacity-50"
                                >
                                    {isUnpublishingThis ? '下线中…' : '下线'}
                                </button>
                            ) : null}
                            <button
                                type="button"
                                aria-expanded={isExpanded}
                                onClick={() =>
                                    setExpandedItemId((prev) =>
                                        prev === item.post_id ? null : item.post_id,
                                    )
                                }
                                title="调整目录短标题与所属篇章"
                                aria-label={`编辑 ${item.title} 的目录设置`}
                                className={cn(
                                    'rounded-md p-1.5 transition-colors',
                                    isExpanded
                                        ? 'bg-accent-tint text-accent'
                                        : 'text-ui-sub hover:bg-ui-bg hover:text-ui-text',
                                )}
                            >
                                <SlidersHorizontal className="h-3.5 w-3.5" />
                            </button>
                            <button
                                type="button"
                                disabled={indexInGroup === 0}
                                onClick={() =>
                                    handleMoveItemWithinGroup(groupKey, indexInGroup, -1)
                                }
                                title="上移"
                                aria-label={`上移 ${item.title}`}
                                className="rounded-md p-1.5 text-ui-sub hover:bg-ui-bg hover:text-ui-text disabled:opacity-35"
                            >
                                <ArrowUp className="h-3.5 w-3.5" />
                            </button>
                            <button
                                type="button"
                                disabled={indexInGroup === groupLength - 1}
                                onClick={() => handleMoveItemWithinGroup(groupKey, indexInGroup, 1)}
                                title="下移"
                                aria-label={`下移 ${item.title}`}
                                className="rounded-md p-1.5 text-ui-sub hover:bg-ui-bg hover:text-ui-text disabled:opacity-35"
                            >
                                <ArrowDown className="h-3.5 w-3.5" />
                            </button>
                            <button
                                type="button"
                                onClick={() => handleRemoveItem(groupKey, item.post_id)}
                                title="移出专栏"
                                aria-label={`移出 ${item.title}`}
                                className="rounded-md p-1.5 text-ui-sub hover:bg-bad-tint hover:text-bad"
                            >
                                <X className="h-3.5 w-3.5" />
                            </button>
                        </div>
                    </div>

                    {isExpanded ? (
                        <div className="grid gap-2 border-t border-ui-line/60 pt-2.5 pl-9 sm:grid-cols-[minmax(0,1fr)_180px]">
                            <div>
                                <label
                                    className="sr-only"
                                    htmlFor={`display-title-${item.post_id}`}
                                >
                                    目录短标题
                                </label>
                                <input
                                    id={`display-title-${item.post_id}`}
                                    type="text"
                                    maxLength={200}
                                    value={item.display_title}
                                    onChange={(event) =>
                                        handleUpdateDisplayTitle(
                                            groupKey,
                                            item.post_id,
                                            event.target.value,
                                        )
                                    }
                                    placeholder={`目录短标题（留空则显示「${item.title}」）`}
                                    className="studio-input h-8 w-full text-[12.5px]"
                                />
                            </div>
                            <Select
                                id={`move-group-${item.post_id}`}
                                ariaLabel="移动到分组"
                                value={groupKey}
                                onChange={(nextGroup) =>
                                    handleMoveItemAcrossGroups(groupKey, item.post_id, nextGroup)
                                }
                                options={groupOptions.map((opt) => ({
                                    value: opt.key,
                                    label: opt.label,
                                }))}
                            />
                        </div>
                    ) : null}
                </div>
            </li>
        );
    };

    return (
        <PageContainer className="space-y-4">
            {/* 返回上一页 */}
            <div>
                <Link
                    to="/series"
                    className="-ml-2 inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[13px] font-medium text-ui-sub transition-colors hover:bg-white hover:text-ui-text"
                >
                    <ArrowLeft className="h-3.5 w-3.5" />
                    返回专栏列表
                </Link>
            </div>

            {/* 顶部专栏概览与操作栏 */}
            <header className="studio-card flex flex-wrap items-start justify-between gap-4 px-5 py-4">
                <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                        <h1 className="truncate text-[18px] font-semibold text-ui-text">
                            {series.title}
                        </h1>
                        <StatusChip tone={pubStatus.tone}>{pubStatus.label}</StatusChip>
                        <StatusChip tone={compStatus.tone}>{compStatus.label}</StatusChip>
                    </div>
                    <p className="tnum mt-1 text-[12px] text-ui-sub">
                        <span className="font-code">/series/{series.slug}/</span> · 编辑版本 v
                        {series.edit_version} · 已发布 {series.published_post_count} / 目录共{' '}
                        {series.item_count} 篇
                    </p>
                    {series.description ? (
                        <p className="mt-2 max-w-[780px] text-[13px] leading-relaxed text-ui-text/80">
                            {series.description}
                        </p>
                    ) : null}
                    <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px] text-ui-sub">
                        <span>{pubStatus.description}</span>
                        {series.first_published_at ? (
                            <span className="tnum">
                                首次公开：{formatDateTime(series.first_published_at)}
                            </span>
                        ) : null}
                        <span>线上状态：{series.is_live ? '在线' : '未在线'}</span>
                    </div>
                </div>

                <div className="flex shrink-0 flex-wrap items-center gap-2">
                    <Button variant="secondary" size="sm" onClick={() => setMetaDrawerOpen(true)}>
                        <SlidersHorizontal className="h-3.5 w-3.5" />
                        基础信息
                        {metaDirty ? (
                            <span
                                className="h-1.5 w-1.5 rounded-full bg-warn"
                                aria-label="有未保存修改"
                            />
                        ) : null}
                    </Button>

                    {canPublish ? (
                        series.published_at ? (
                            <Button
                                variant="danger-outline"
                                size="sm"
                                onClick={() => setUnpublishModalOpen(true)}
                            >
                                取消公开
                            </Button>
                        ) : (
                            <Button
                                variant="primary"
                                size="sm"
                                onClick={() => setPublishModalOpen(true)}
                            >
                                <Globe className="h-3.5 w-3.5" />
                                公开系列
                            </Button>
                        )
                    ) : null}

                    <Button
                        variant="ghost"
                        size="sm"
                        disabled={!deletable}
                        title={
                            deletable
                                ? '删除该系列（不会删除关联文章）'
                                : '仅当系列期望未公开且已不在线时才可删除'
                        }
                        className="text-bad hover:bg-bad-tint hover:text-bad disabled:text-ui-sub"
                        onClick={() => setDeleteModalOpen(true)}
                    >
                        <Trash2 className="h-3.5 w-3.5" />
                        删除系列
                    </Button>
                </div>
            </header>

            {/* 主工作区：全宽目录大纲编排 */}
            <section
                aria-labelledby="series-outline-heading"
                className="studio-card min-w-0 overflow-hidden"
            >
                <header className="flex flex-wrap items-center justify-between gap-3 border-b border-ui-line bg-ui-subtle/60 px-5 py-3">
                    <div>
                        <div className="flex items-center gap-2">
                            <h2
                                id="series-outline-heading"
                                className="text-[14.5px] font-semibold text-ui-text"
                            >
                                目录大纲编排
                            </h2>
                            <span className="tnum rounded-full bg-white px-2 py-0.5 text-[11.5px] font-medium text-ui-sub shadow-2xs">
                                {outline.sections.length}/{MAX_SECTIONS} 篇章 · {totalOutlineItems}/
                                {MAX_ITEMS} 篇
                            </span>
                            {outlineDirty ? <StatusChip tone="warn">目录未保存</StatusChip> : null}
                        </div>
                        <p className="mt-0.5 text-[12px] text-ui-sub">
                            公开阅读顺序为根目录在前、各篇章依次展开；草稿可先排入大纲，仅已发布文章对外展示。
                        </p>
                    </div>

                    <div className="flex flex-wrap items-center gap-2">
                        {canPublish && publishableItems.length > 0 ? (
                            <Button
                                variant="secondary"
                                size="sm"
                                disabled={batchPublishMutation.isPending}
                                onClick={() => setBatchPublishModalOpen(true)}
                            >
                                <Send className="h-3.5 w-3.5 text-accent" />
                                {batchPublishMutation.isPending
                                    ? '批量发布中…'
                                    : `一键发布 (${publishableItems.length})`}
                            </Button>
                        ) : null}
                        <Button variant="secondary" size="sm" onClick={handleAddSection}>
                            <FolderPlus className="h-3.5 w-3.5" />
                            新增篇章分组
                        </Button>
                        <Button
                            variant="secondary"
                            size="sm"
                            onClick={() => setAddExistingOpen(true)}
                        >
                            <Plus className="h-3.5 w-3.5" />
                            添加已有文章
                        </Button>
                        {canCreatePost ? (
                            <Button
                                variant="secondary"
                                size="sm"
                                onClick={() => setQuickCreateOpen(true)}
                            >
                                <FilePlus2 className="h-3.5 w-3.5" />
                                在此系列新建文章
                            </Button>
                        ) : null}
                        <Button
                            variant="primary"
                            size="sm"
                            disabled={!outlineDirty || saveOutlineMutation.isPending}
                            onClick={() => handleSaveOutline()}
                        >
                            <Save className="h-3.5 w-3.5" />
                            {saveOutlineMutation.isPending ? '保存目录中…' : '保存目录'}
                        </Button>
                    </div>
                </header>

                <div className="space-y-5 p-5">
                    {/* 目录冲突提示：保留本地编排并展示最新目录供核对 */}
                    {outlineConflict ? (
                        <div
                            role="alert"
                            className="space-y-3 rounded-xl border border-warn/40 bg-warn-tint p-4 text-[13px] text-warn"
                        >
                            <div className="flex items-start gap-2.5">
                                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                                <div className="min-w-0 flex-1">
                                    <p className="font-semibold">
                                        目录保存冲突：{outlineConflict.message}
                                    </p>
                                    <p className="mt-0.5 text-[12.5px]">
                                        你的本地编排已完整保留。可先核对下方服务端最新目录，再决定重新提交或放弃本地修改。
                                    </p>
                                </div>
                            </div>

                            {outlineConflict.latest ? (
                                <details className="rounded-lg border border-warn/25 bg-white/85 px-3 py-2 text-ui-text">
                                    <summary className="cursor-pointer text-[12.5px] font-medium">
                                        查看服务端最新目录（v
                                        {outlineConflict.latest.edit_version}，共{' '}
                                        {outlineConflict.latest.item_count} 篇）
                                    </summary>
                                    <div className="mt-2 space-y-2 text-[12px] text-ui-sub">
                                        {outlineConflict.latest.items.length > 0 ? (
                                            <div>
                                                <div className="font-medium text-ui-text">
                                                    根目录
                                                </div>
                                                <ul className="mt-1 list-disc pl-4">
                                                    {outlineConflict.latest.items.map((it) => (
                                                        <li key={it.post_id}>
                                                            {it.display_title ?? it.title} ( /
                                                            {it.slug})
                                                        </li>
                                                    ))}
                                                </ul>
                                            </div>
                                        ) : null}
                                        {outlineConflict.latest.sections.map((sec) => (
                                            <div key={sec.id}>
                                                <div className="font-medium text-ui-text">
                                                    篇章：{sec.title}
                                                </div>
                                                <ul className="mt-1 list-disc pl-4">
                                                    {sec.items.map((it) => (
                                                        <li key={it.post_id}>
                                                            {it.display_title ?? it.title} ( /
                                                            {it.slug})
                                                        </li>
                                                    ))}
                                                </ul>
                                            </div>
                                        ))}
                                        {outlineConflict.latest.item_count === 0 ? (
                                            <p>服务端目录当前为空。</p>
                                        ) : null}
                                    </div>
                                </details>
                            ) : null}

                            <div className="flex flex-wrap gap-2">
                                {outlineConflict.latest ? (
                                    <Button
                                        variant="secondary"
                                        size="sm"
                                        onClick={() => {
                                            setOutline(
                                                seriesToOutlineDraft(outlineConflict.latest!),
                                            );
                                            setOutlineDirty(false);
                                            setOutlineConflict(null);
                                            toast('已采用服务端最新目录', 'info');
                                        }}
                                    >
                                        采用最新目录
                                    </Button>
                                ) : null}
                                {outlineConflict.code === 'series_edit_conflict' &&
                                outlineConflict.latest ? (
                                    <Button
                                        variant="primary"
                                        size="sm"
                                        disabled={saveOutlineMutation.isPending}
                                        onClick={() =>
                                            handleSaveOutline(outlineConflict.latest!.edit_version)
                                        }
                                    >
                                        以最新版本号（v{outlineConflict.latest.edit_version}
                                        ）重新提交本地目录
                                    </Button>
                                ) : null}
                            </div>
                        </div>
                    ) : null}

                    {/* 根目录文章分组 */}
                    <div
                        onDragOver={(event) => event.preventDefault()}
                        onDrop={(event) => {
                            event.preventDefault();
                            if (!dragPayload) return;
                            handleMoveItemAcrossGroups(
                                dragPayload.groupKey,
                                dragPayload.postId,
                                '__root__',
                            );
                            setDragPayload(null);
                        }}
                        className="rounded-xl border border-ui-line bg-ui-subtle/40 p-4"
                    >
                        <div className="mb-3 flex items-center justify-between gap-2">
                            <div>
                                <h3 className="text-[13.5px] font-semibold text-ui-text">
                                    根目录文章（直接挂在系列下，排在各篇章之前）
                                </h3>
                                <p className="text-[12px] text-ui-sub">
                                    短系列可直接全部放在根目录，无需创建篇章分组。
                                </p>
                            </div>
                            <span className="tnum text-[12px] text-ui-sub">
                                {rootWithOrder.length} 篇
                            </span>
                        </div>

                        {rootWithOrder.length === 0 ? (
                            <p className="rounded-lg border border-dashed border-ui-line bg-white/70 px-4 py-6 text-center text-[13px] text-ui-sub">
                                根目录暂无文章。可点击上方「添加已有文章」或拖拽条目至此。
                            </p>
                        ) : (
                            <ul className="space-y-2">
                                {rootWithOrder.map(({ item, order }, idx) =>
                                    renderItemRow(
                                        '__root__',
                                        item,
                                        idx,
                                        rootWithOrder.length,
                                        order,
                                    ),
                                )}
                            </ul>
                        )}
                    </div>

                    {/* 篇章分组列表 */}
                    {sectionsWithOrder.map(({ section, items }, sectionIndex) => (
                        <div
                            key={section.localKey}
                            onDragOver={(event) => event.preventDefault()}
                            onDrop={(event) => {
                                event.preventDefault();
                                if (!dragPayload) return;
                                handleMoveItemAcrossGroups(
                                    dragPayload.groupKey,
                                    dragPayload.postId,
                                    section.localKey,
                                );
                                setDragPayload(null);
                            }}
                            className="rounded-xl border border-ui-line bg-ui-subtle/40 p-4"
                        >
                            <div className="mb-3 flex flex-wrap items-start gap-2.5">
                                <div className="grid min-w-0 flex-1 gap-2 sm:grid-cols-2">
                                    <div>
                                        <label
                                            className="sr-only"
                                            htmlFor={`sec-title-${section.localKey}`}
                                        >
                                            篇章标题
                                        </label>
                                        <input
                                            id={`sec-title-${section.localKey}`}
                                            type="text"
                                            maxLength={100}
                                            value={section.title}
                                            onChange={(event) =>
                                                patchOutline((prev) => ({
                                                    ...prev,
                                                    sections: prev.sections.map((s) =>
                                                        s.localKey === section.localKey
                                                            ? {
                                                                  ...s,
                                                                  title: event.target.value,
                                                              }
                                                            : s,
                                                    ),
                                                }))
                                            }
                                            placeholder="篇章标题（必填，1–100 字符）"
                                            className="studio-input h-9 w-full font-medium text-[13.5px]"
                                        />
                                    </div>
                                    <div>
                                        <label
                                            className="sr-only"
                                            htmlFor={`sec-desc-${section.localKey}`}
                                        >
                                            篇章简介
                                        </label>
                                        <input
                                            id={`sec-desc-${section.localKey}`}
                                            type="text"
                                            maxLength={500}
                                            value={section.description}
                                            onChange={(event) =>
                                                patchOutline((prev) => ({
                                                    ...prev,
                                                    sections: prev.sections.map((s) =>
                                                        s.localKey === section.localKey
                                                            ? {
                                                                  ...s,
                                                                  description: event.target.value,
                                                              }
                                                            : s,
                                                    ),
                                                }))
                                            }
                                            placeholder="篇章简介（可选）"
                                            className="studio-input h-9 w-full text-[13px]"
                                        />
                                    </div>
                                </div>

                                <div className="flex shrink-0 items-center gap-1">
                                    <span className="tnum mr-1 text-[12px] text-ui-sub">
                                        {items.length} 篇
                                    </span>
                                    <button
                                        type="button"
                                        disabled={sectionIndex === 0}
                                        onClick={() => handleMoveSection(sectionIndex, -1)}
                                        title="篇章上移"
                                        aria-label={`上移篇章 ${section.title}`}
                                        className="rounded-md p-1.5 text-ui-sub hover:bg-white hover:text-ui-text disabled:opacity-35"
                                    >
                                        <ArrowUp className="h-3.5 w-3.5" />
                                    </button>
                                    <button
                                        type="button"
                                        disabled={sectionIndex === sectionsWithOrder.length - 1}
                                        onClick={() => handleMoveSection(sectionIndex, 1)}
                                        title="篇章下移"
                                        aria-label={`下移篇章 ${section.title}`}
                                        className="rounded-md p-1.5 text-ui-sub hover:bg-white hover:text-ui-text disabled:opacity-35"
                                    >
                                        <ArrowDown className="h-3.5 w-3.5" />
                                    </button>
                                    <button
                                        type="button"
                                        onClick={() => {
                                            if (section.items.length === 0) {
                                                handleRemoveSection(section, false);
                                            } else {
                                                setDeleteSectionTarget(section);
                                            }
                                        }}
                                        title="删除篇章"
                                        aria-label={`删除篇章 ${section.title}`}
                                        className="rounded-md p-1.5 text-ui-sub hover:bg-bad-tint hover:text-bad"
                                    >
                                        <Trash2 className="h-3.5 w-3.5" />
                                    </button>
                                </div>
                            </div>

                            {items.length === 0 ? (
                                <p className="rounded-lg border border-dashed border-ui-line bg-white/70 px-4 py-5 text-center text-[12.5px] text-ui-sub">
                                    此篇章暂无文章（公开站点会自动省略空篇章）。可添加文章或拖拽条目至此。
                                </p>
                            ) : (
                                <ul className="space-y-2">
                                    {items.map(({ item, order }, idx) =>
                                        renderItemRow(
                                            section.localKey,
                                            item,
                                            idx,
                                            items.length,
                                            order,
                                        ),
                                    )}
                                </ul>
                            )}
                        </div>
                    ))}
                </div>
            </section>

            {/* 专栏基础信息抽屉 */}
            <Drawer
                open={metaDrawerOpen}
                onClose={closeMetaDrawer}
                title="专栏基础信息"
                subtitle="保存后同步更新公开专栏"
                widthClassName="sm:w-[480px]"
                footer={
                    <div className="flex items-center justify-between gap-3">
                        <div>{metaDirty ? <StatusChip tone="warn">未保存</StatusChip> : null}</div>
                        <div className="flex items-center gap-2">
                            <Button variant="secondary" onClick={closeMetaDrawer}>
                                关闭
                            </Button>
                            <Button
                                variant="primary"
                                disabled={!metaDirty || saveMetaMutation.isPending}
                                onClick={handleSaveMetadata}
                            >
                                <Save className="h-4 w-4" />
                                {saveMetaMutation.isPending ? '保存中…' : '保存基础信息'}
                            </Button>
                        </div>
                    </div>
                }
            >
                <div className="space-y-4 text-[14px]">
                    {metaConflict ? (
                        <div
                            role="alert"
                            className="rounded-lg border border-warn/40 bg-warn-tint p-3 text-[12.5px] text-warn"
                        >
                            <p className="font-medium">
                                检测到服务端编辑版本已更新，你的本地表单输入仍保留。
                            </p>
                            <div className="mt-2 flex flex-wrap gap-2">
                                <Button
                                    variant="secondary"
                                    size="sm"
                                    onClick={async () => {
                                        const latest = await handleRefreshSeries();
                                        setMetaForm(seriesToMetadataForm(latest));
                                        setMetaDirty(false);
                                        setMetaConflict(false);
                                        toast('已载入最新基础信息', 'info');
                                    }}
                                >
                                    载入最新资料
                                </Button>
                                <Button
                                    variant="primary"
                                    size="sm"
                                    onClick={async () => {
                                        const latest = await handleRefreshSeries();
                                        setMetaConflict(false);
                                        saveMetaMutation.mutate({
                                            form: metaForm,
                                            current: latest,
                                        });
                                    }}
                                >
                                    用最新版本号重试保存
                                </Button>
                            </div>
                        </div>
                    ) : null}

                    <div>
                        <label htmlFor="wb-title" className="block font-medium text-ui-text">
                            专栏标题 <span className="text-bad">*</span>
                        </label>
                        <input
                            id="wb-title"
                            type="text"
                            maxLength={100}
                            value={metaForm.title}
                            onChange={(event) => {
                                setMetaForm((prev) =>
                                    prev ? { ...prev, title: event.target.value } : prev,
                                );
                                setMetaDirty(true);
                            }}
                            className="studio-input mt-1.5"
                        />
                    </div>

                    <div>
                        <label htmlFor="wb-slug" className="block font-medium text-ui-text">
                            Slug
                        </label>
                        <input
                            id="wb-slug"
                            type="text"
                            maxLength={100}
                            readOnly={series.slug_locked}
                            value={metaForm.slug}
                            onChange={(event) => {
                                if (series.slug_locked) return;
                                setMetaForm((prev) =>
                                    prev ? { ...prev, slug: event.target.value } : prev,
                                );
                                setMetaDirty(true);
                            }}
                            className={cn(
                                'studio-input mt-1.5 font-code text-[13px]',
                                series.slug_locked && 'bg-ui-subtle text-ui-sub',
                            )}
                        />
                        <p className="mt-1 text-[12px] text-ui-sub">
                            {series.slug_locked
                                ? '首次受理公开后 Slug 已锁定，避免公开专栏链接失效。'
                                : '未公开前可修改，仅允许小写字母、数字和单个连字符。'}
                        </p>
                    </div>

                    <div>
                        <label htmlFor="wb-category" className="block font-medium text-ui-text">
                            所属主分类
                        </label>
                        <div className="mt-1.5">
                            <Select
                                id="wb-category"
                                ariaLabel="所属主分类"
                                className="w-full"
                                value={metaForm.category_id ?? ''}
                                onChange={(nextValue) => {
                                    setMetaForm((prev) =>
                                        prev ? { ...prev, category_id: nextValue || null } : prev,
                                    );
                                    setMetaDirty(true);
                                }}
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

                    {/* 封面 */}
                    <div>
                        <span className="block font-medium text-ui-text">专栏封面</span>
                        <div className="mt-1.5 flex items-center gap-3 rounded-lg border border-dashed border-ui-line p-2.5">
                            <div className="h-14 w-24 shrink-0 overflow-hidden rounded border border-ui-line bg-[#EEF0F3]">
                                {metaForm.cover_media_id ? (
                                    <AuthenticatedImage
                                        mediaId={metaForm.cover_media_id}
                                        alt={metaForm.title}
                                        className="h-full w-full object-cover"
                                    />
                                ) : (
                                    <div className="grid h-full w-full place-items-center text-ui-sub">
                                        <BookOpen className="h-5 w-5 opacity-40" />
                                    </div>
                                )}
                            </div>
                            <div className="min-w-0 flex-1">
                                <div className="truncate font-code text-[11.5px] text-ui-sub">
                                    {metaForm.cover_media_id ?? '未设置封面（使用默认样式）'}
                                </div>
                                <div className="mt-1.5 flex gap-3 text-[13px]">
                                    <button
                                        type="button"
                                        onClick={() => setCoverPickerOpen(true)}
                                        className="font-medium text-accent hover:underline"
                                    >
                                        {metaForm.cover_media_id ? '更换封面' : '选择媒体'}
                                    </button>
                                    {metaForm.cover_media_id ? (
                                        <button
                                            type="button"
                                            onClick={() => {
                                                setMetaForm((prev) =>
                                                    prev ? { ...prev, cover_media_id: null } : prev,
                                                );
                                                setMetaDirty(true);
                                            }}
                                            className="text-ui-sub hover:text-bad"
                                        >
                                            移除
                                        </button>
                                    ) : null}
                                </div>
                            </div>
                        </div>
                    </div>

                    <div>
                        <div className="flex justify-between">
                            <label htmlFor="wb-desc" className="block font-medium text-ui-text">
                                专栏简介
                            </label>
                            <span className="tnum text-[12px] text-ui-sub">
                                {metaForm.description.length}/1000
                            </span>
                        </div>
                        <textarea
                            id="wb-desc"
                            rows={3}
                            maxLength={1000}
                            value={metaForm.description}
                            onChange={(event) => {
                                setMetaForm((prev) =>
                                    prev ? { ...prev, description: event.target.value } : prev,
                                );
                                setMetaDirty(true);
                            }}
                            placeholder="展示在专栏总览卡片与专栏头部（可选）"
                            className="studio-input mt-1.5 h-auto py-2 text-[13px]"
                        />
                    </div>

                    <div className="grid grid-cols-2 gap-3">
                        <div>
                            <label
                                htmlFor="wb-completion"
                                className="block font-medium text-ui-text"
                            >
                                连载状态
                            </label>
                            <div className="mt-1.5">
                                <Select
                                    id="wb-completion"
                                    ariaLabel="连载状态"
                                    className="w-full"
                                    value={metaForm.completion_status}
                                    onChange={(nextValue) => {
                                        setMetaForm((prev) =>
                                            prev
                                                ? {
                                                      ...prev,
                                                      completion_status: nextValue as
                                                          | 'ongoing'
                                                          | 'completed',
                                                  }
                                                : prev,
                                        );
                                        setMetaDirty(true);
                                    }}
                                    options={[
                                        { value: 'ongoing', label: '连载中' },
                                        { value: 'completed', label: '已完结' },
                                    ]}
                                />
                            </div>
                        </div>
                        <div>
                            <label
                                htmlFor="wb-sort-order"
                                className="block font-medium text-ui-text"
                            >
                                排序权重
                            </label>
                            <input
                                id="wb-sort-order"
                                type="number"
                                value={metaForm.sort_order}
                                onChange={(event) => {
                                    setMetaForm((prev) =>
                                        prev ? { ...prev, sort_order: event.target.value } : prev,
                                    );
                                    setMetaDirty(true);
                                }}
                                className="studio-input mt-1.5 font-code text-[13px]"
                            />
                        </div>
                    </div>

                    <label
                        htmlFor="wb-featured"
                        aria-label="精选专栏"
                        className="flex items-start gap-2.5"
                    >
                        <input
                            id="wb-featured"
                            type="checkbox"
                            checked={metaForm.is_featured}
                            onChange={(event) => {
                                setMetaForm((prev) =>
                                    prev ? { ...prev, is_featured: event.target.checked } : prev,
                                );
                                setMetaDirty(true);
                            }}
                            className="mt-0.5 h-4 w-4 accent-[#137A4D]"
                        />
                        <span>
                            <span className="font-medium text-ui-text">精选专栏</span>
                            <span className="block text-[12px] text-ui-sub">
                                在总览页卡片上标注「精选」，不改变排序权重顺序。
                            </span>
                        </span>
                    </label>

                    <fieldset className="space-y-3 border-t border-ui-line pt-4">
                        <legend className="font-medium text-ui-text">SEO 设置</legend>
                        <div>
                            <label htmlFor="wb-seo-title" className="block text-[13px] text-ui-sub">
                                SEO 标题（留空则使用专栏标题）
                            </label>
                            <input
                                id="wb-seo-title"
                                type="text"
                                maxLength={200}
                                value={metaForm.seo_title}
                                onChange={(event) => {
                                    setMetaForm((prev) =>
                                        prev ? { ...prev, seo_title: event.target.value } : prev,
                                    );
                                    setMetaDirty(true);
                                }}
                                placeholder={metaForm.title || '专栏标题'}
                                className="studio-input mt-1 text-[13px]"
                            />
                        </div>
                        <div>
                            <div className="flex justify-between">
                                <label
                                    htmlFor="wb-seo-desc"
                                    className="block text-[13px] text-ui-sub"
                                >
                                    SEO 描述
                                </label>
                                <span className="tnum text-[12px] text-ui-sub">
                                    {metaForm.seo_description.length}/500
                                </span>
                            </div>
                            <textarea
                                id="wb-seo-desc"
                                rows={2}
                                maxLength={500}
                                value={metaForm.seo_description}
                                onChange={(event) => {
                                    setMetaForm((prev) =>
                                        prev
                                            ? { ...prev, seo_description: event.target.value }
                                            : prev,
                                    );
                                    setMetaDirty(true);
                                }}
                                className="studio-input mt-1 h-auto py-2 text-[13px]"
                            />
                        </div>
                    </fieldset>
                </div>
            </Drawer>

            {/* 封面选择器 */}
            <MediaPickerDrawer
                open={coverPickerOpen}
                onClose={() => setCoverPickerOpen(false)}
                title="选择系列封面"
                confirmLabel="设为系列封面"
                onSelect={(media) => {
                    setMetaForm((prev) => (prev ? { ...prev, cover_media_id: media.id } : prev));
                    setMetaDirty(true);
                }}
            />

            {/* 添加已有文章弹窗 */}
            <AddExistingPostsModal
                open={addExistingOpen}
                onClose={() => setAddExistingOpen(false)}
                existingPostIds={existingPostIds}
                groupOptions={groupOptions}
                onAddPosts={handleAddExistingPosts}
            />

            {/* 在此系列新建文章弹窗 */}
            <QuickCreateSeriesPostModal
                open={quickCreateOpen}
                onClose={() => setQuickCreateOpen(false)}
                series={series}
                outlineDirty={outlineDirty}
                groupOptions={groupOptions}
                onSaveOutlineFirst={handleSaveOutlineForQuickCreate}
                onRefreshSeries={handleRefreshSeries}
                onCompleted={async (result) => {
                    if (result.publication)
                        void queryClient.invalidateQueries({
                            queryKey: queryKeys.publicSite.current,
                        });
                    void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
                    void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
                    const latest = await handleRefreshSeries();
                    setOutline(seriesToOutlineDraft(latest));
                    setOutlineDirty(false);
                    toast(`已新建「${result.post.current_revision.title}」并加入系列`, 'ok');
                }}
            />

            {/* 删除非空篇章确认弹窗 */}
            <Modal
                open={deleteSectionTarget !== null}
                onClose={() => setDeleteSectionTarget(null)}
                title={`删除篇章「${deleteSectionTarget?.title ?? ''}」？`}
                description={`该篇章下包含 ${deleteSectionTarget?.items.length ?? 0} 篇文章，请选择如何处理这些文章：`}
                footer={
                    deleteSectionTarget ? (
                        <div className="flex flex-wrap justify-end gap-2">
                            <Button
                                variant="secondary"
                                onClick={() => setDeleteSectionTarget(null)}
                            >
                                取消
                            </Button>
                            <Button
                                variant="danger-outline"
                                onClick={() => handleRemoveSection(deleteSectionTarget, false)}
                            >
                                移出系列并删篇章
                            </Button>
                            <Button
                                variant="primary"
                                onClick={() => handleRemoveSection(deleteSectionTarget, true)}
                            >
                                移至根目录并删篇章
                            </Button>
                        </div>
                    ) : null
                }
            >
                <p className="text-[13px] text-ui-sub">
                    此操作仅调整本地目录缓冲，点击「保存目录」后才会提交到服务端。文章本体和修订均不会被删除。
                </p>
            </Modal>

            {/* 公开系列确认弹窗 */}
            <Modal
                open={publishModalOpen}
                onClose={() => setPublishModalOpen(false)}
                title={`公开系列「${series.title}」？`}
                description="首次公开后将锁定系列的 Slug。公开目录仅展示已发布文章。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setPublishModalOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={publishMutation.isPending}
                            onClick={() => publishMutation.mutate(series.publication_version)}
                        >
                            {publishMutation.isPending ? '提交中…' : '确认公开'}
                        </Button>
                    </>
                }
            >
                <div className="space-y-1.5 text-[13px] text-ui-sub">
                    <p>
                        当前目录共 {series.item_count} 篇，其中已发布文章{' '}
                        <strong className="text-ui-text">{series.published_post_count}</strong> 篇。
                    </p>
                    {series.published_post_count === 0 ? (
                        <p className="text-warn">
                            提示：目前没有已发布的成员文章，公开受理后暂不会生成公开专栏页，待至少有一篇成员文章发布后自动呈现。
                        </p>
                    ) : null}
                </div>
            </Modal>

            {/* 取消公开确认弹窗 */}
            <Modal
                open={unpublishModalOpen}
                onClose={() => setUnpublishModalOpen(false)}
                role="alertdialog"
                title={`取消公开系列「${series.title}」？`}
                description="下线后，专栏页返回 404，系列总览与 sitemap 将移除该专栏。成员文章仍可通过原链接访问。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setUnpublishModalOpen(false)}>
                            保留公开
                        </Button>
                        <Button
                            variant="danger"
                            disabled={unpublishMutation.isPending}
                            onClick={() => unpublishMutation.mutate(series.publication_version)}
                        >
                            {unpublishMutation.isPending ? '提交中…' : '确认取消公开'}
                        </Button>
                    </>
                }
            >
                <p className="text-[13px] text-ui-sub">专栏目录会保留在后台，以后可以重新公开。</p>
            </Modal>

            {/* 取消发布单篇文章确认弹窗 */}
            <Modal
                open={unpublishPostTarget !== null}
                onClose={() => setUnpublishPostTarget(null)}
                role="alertdialog"
                title={`取消发布「${unpublishPostTarget?.title ?? ''}」？`}
                description="下线后，公开站点和专栏目录将移除这篇文章。草稿和后台目录位置会保留。"
                footer={
                    unpublishPostTarget ? (
                        <>
                            <Button
                                variant="secondary"
                                onClick={() => setUnpublishPostTarget(null)}
                            >
                                保留在线
                            </Button>
                            <Button
                                variant="danger"
                                disabled={unpublishPostMutation.isPending}
                                onClick={() => unpublishPostMutation.mutate(unpublishPostTarget)}
                            >
                                {unpublishPostMutation.isPending ? '提交中…' : '确认下线'}
                            </Button>
                        </>
                    ) : null
                }
            >
                {unpublishPostTarget ? (
                    <p className="font-code text-[12.5px] text-ui-sub">
                        /blog/{unpublishPostTarget.slug}/
                    </p>
                ) : null}
            </Modal>

            {/* 一键批量发布专栏内未发布/有更新文章确认弹窗 */}
            <Modal
                open={batchPublishModalOpen}
                onClose={() => {
                    if (!batchPublishMutation.isPending) setBatchPublishModalOpen(false);
                }}
                title={`一键发布 ${publishableItems.length} 篇文章？`}
                description="将依次发布本专栏中尚未公开或有新草稿的文章。"
                maxWidthClassName="max-w-[520px]"
                footer={
                    <>
                        <Button
                            variant="secondary"
                            disabled={batchPublishMutation.isPending}
                            onClick={() => setBatchPublishModalOpen(false)}
                        >
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={
                                publishableItems.length === 0 || batchPublishMutation.isPending
                            }
                            onClick={() => batchPublishMutation.mutate([...publishableItems])}
                        >
                            <Send className="h-3.5 w-3.5" />
                            {batchPublishMutation.isPending
                                ? '发布中…'
                                : `确认发布（${publishableItems.length} 篇）`}
                        </Button>
                    </>
                }
            >
                <div className="space-y-3 text-[13px]">
                    <ul className="max-h-56 space-y-1.5 overflow-y-auto rounded-lg border border-ui-line bg-ui-subtle/50 p-3">
                        {publishableItems.map((item, idx) => (
                            <li
                                key={item.post_id}
                                className="flex items-center justify-between gap-2 text-ui-text"
                            >
                                <span className="truncate">
                                    <span className="tnum mr-1.5 font-code text-[11.5px] text-ui-sub">
                                        {String(idx + 1).padStart(2, '0')}
                                    </span>
                                    {item.title}
                                </span>
                                <span className="shrink-0 text-[11.5px] text-ui-sub">
                                    {resolveItemStatus(item).published_revision_id
                                        ? '更新发布'
                                        : '首次发布'}
                                </span>
                            </li>
                        ))}
                    </ul>
                    {!series.published_at ? (
                        <p className="text-[12.5px] text-warn">
                            提示：当前专栏尚未对外公开，文章发布后可点击顶部「公开系列」让专栏页上线展示。
                        </p>
                    ) : null}
                </div>
            </Modal>

            {/* 删除系列确认弹窗 */}
            <Modal
                open={deleteModalOpen}
                onClose={() => setDeleteModalOpen(false)}
                role="alertdialog"
                title={`删除系列「${series.title}」？`}
                description="删除后无法恢复该系列的元数据、篇章和目录编排，但所有文章与修订都会完整保留。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setDeleteModalOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="danger"
                            disabled={deleteMutation.isPending}
                            onClick={() => deleteMutation.mutate(series)}
                        >
                            {deleteMutation.isPending ? '删除中…' : '确认删除'}
                        </Button>
                    </>
                }
            >
                <p className="font-code text-[13px] text-ui-sub">/series/{series.slug}/</p>
            </Modal>
        </PageContainer>
    );
}
