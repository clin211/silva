import { websiteUrl } from '@/lib/website';
import { postPublicationStatus } from './post-publication-status';
import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import {
    ArrowLeft,
    BookOpen,
    Columns2,
    ExternalLink,
    PenLine,
    SlidersHorizontal,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TiptapMarkdownEditor, type TiptapMarkdownEditorHandle } from './tiptap-markdown-editor';
import { AuthenticatedImage, SafeHtmlPreview } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import { ErrorBanner, LoadingCard } from '@/components/ui/feedback';
import { Drawer, Modal } from '@/components/ui/overlay';
import { Select } from '@/components/ui/select';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { MediaPickerDrawer } from '@/features/media/media-picker-drawer';
import { apiClient, unwrapResponse, uploadMediaFile } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId, getMediaRejectionMessage, isApiError } from '@/lib/errors';
import { formatRelativeTime, utf8ByteLength } from '@/lib/format';
import { renderSafeMarkdown } from '@/lib/markdown';
import { hasAnyPermission, hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

interface DraftFields {
    title: string;
    slug: string;
    excerpt: string;
    markdown_body: string;
    category_ids: string[];
    tag_ids: string[];
    cover_media_id: string | null;
    is_featured: boolean;
    seo_title: string;
    seo_description: string;
}

type SaveState =
    | { kind: 'idle'; text: string }
    | { kind: 'dirty'; text: string }
    | { kind: 'saving'; text: string }
    | { kind: 'saved'; text: string }
    | { kind: 'failed'; text: string }
    | { kind: 'conflict'; text: string };

const EMPTY_DRAFT: DraftFields = {
    title: '',
    slug: '',
    excerpt: '',
    markdown_body: '',
    category_ids: [],
    tag_ids: [],
    cover_media_id: null,
    is_featured: false,
    seo_title: '',
    seo_description: '',
};

const SLUG_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const ALLOWED_IMAGE_MIME_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp']);
const MAX_IMAGE_UPLOAD_BYTES = 10 * 1024 * 1024;

function normalizePastedImageFile(file: File): File {
    const trimmedName = file.name.trim();
    if (trimmedName) return file;
    const ext = file.type === 'image/jpeg' ? 'jpg' : file.type === 'image/webp' ? 'webp' : 'png';
    return new File([file], `pasted-image-${Date.now()}.${ext}`, {
        type: file.type || 'image/png',
    });
}

function isValidSlug(value: string): boolean {
    const trimmed = value.trim();
    return trimmed.length > 0 && trimmed.length <= 100 && SLUG_PATTERN.test(trimmed);
}

function postToDraft(post: Schemas['Post']): DraftFields {
    const rev = post.current_revision;
    return {
        title: rev.title,
        slug: post.slug,
        excerpt: rev.excerpt ?? '',
        markdown_body: rev.markdown_body,
        category_ids: rev.categories.map((c) => c.id),
        tag_ids: rev.tags.map((t) => t.id),
        cover_media_id: rev.cover_media_id,
        is_featured: rev.is_featured,
        seo_title: rev.seo_title ?? '',
        seo_description: rev.seo_description ?? '',
    };
}

function toRevisionBody(draft: DraftFields) {
    return {
        title: draft.title.trim(),
        excerpt: draft.excerpt.trim() || null,
        markdown_body: draft.markdown_body,
        category_ids: draft.category_ids,
        tag_ids: draft.tag_ids,
        cover_media_id: draft.cover_media_id,
        is_featured: draft.is_featured,
        seo_title: draft.seo_title.trim() || null,
        seo_description: draft.seo_description.trim() || null,
    };
}

export function PostEditorPage({
    postId,
    initialCategoryId,
}: {
    postId?: string;
    initialCategoryId?: string;
}) {
    const isCreateMode = !postId;
    const navigate = useNavigate();
    const queryClient = useQueryClient();
    const { toast } = useToast();
    const { user } = useAuth();

    const canPublish = hasPermission(user, PERMISSIONS.POSTS_PUBLISH);
    const canManageSeries = hasPermission(user, PERMISSIONS.SERIES_MANAGE);
    const canUploadMedia = hasAnyPermission(user, [
        PERMISSIONS.MEDIA_MANAGE_OWN,
        PERMISSIONS.MEDIA_MANAGE,
    ]);

    const editorRef = useRef<TiptapMarkdownEditorHandle>(null);
    const titleInputRef = useRef<HTMLInputElement>(null);
    const slugInputRef = useRef<HTMLInputElement>(null);
    const editSequence = useRef(0);
    const activeSave = useRef<{ sequence: number; result: Promise<Schemas['Revision']> } | null>(
        null,
    );
    const publishing = useRef(false);
    const [isPublishing, setIsPublishing] = useState(false);
    const [uploadingImages, setUploadingImages] = useState(0);
    const hydratedPostIdRef = useRef<string | null>(null);
    const confirmedLeaveRef = useRef(false);

    const [draft, setDraft] = useState<DraftFields>(() =>
        !postId && initialCategoryId
            ? { ...EMPTY_DRAFT, category_ids: [initialCategoryId] }
            : EMPTY_DRAFT,
    );
    const [expectedRevisionId, setExpectedRevisionId] = useState<string | null>(null);

    const [isDirty, setIsDirty] = useState(false);
    const [hasConflict, setHasConflict] = useState(false);
    const [saveState, setSaveState] = useState<SaveState>({
        kind: 'idle',
        text: isCreateMode ? '草稿尚未创建' : '正在读取文章…',
    });

    const [mobileTab, setMobileTab] = useState<'source' | 'preview'>('source');
    const [previewOpen, setPreviewOpen] = useState(false);
    const [settingsOpen, setSettingsOpen] = useState(false);
    const [slugError, setSlugError] = useState<string | null>(null);
    const [leaveModalOpen, setLeaveModalOpen] = useState(false);
    const [pickerMode, setPickerMode] = useState<'body' | 'cover' | null>(null);
    const [unpublishModalOpen, setUnpublishModalOpen] = useState(false);
    const [selectedSeriesId, setSelectedSeriesId] = useState<string>('');
    const hasContentToConfirmLeave = isCreateMode
        ? draft.title.trim().length > 0 || draft.markdown_body.trim().length > 0
        : isDirty;

    const postQuery = useQuery({
        queryKey: queryKeys.posts.detail(postId ?? ''),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}', {
                    params: { path: { id: postId! } },
                }),
            );
            return res.data;
        },
        enabled: Boolean(postId),
    });

    const categoriesQuery = useQuery({
        queryKey: queryKeys.taxonomy.categories,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/categories')),
    });

    const tagsQuery = useQuery({
        queryKey: queryKeys.taxonomy.tags,
        queryFn: async () => unwrapResponse(apiClient.GET('/api/v1/admin/tags')),
    });

    const seriesOptionsQuery = useQuery({
        queryKey: queryKeys.series.list({ page: 1, per_page: 100 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/series', {
                    params: { query: { page: 1, per_page: 100 } },
                }),
            ),
        enabled: !isCreateMode && canManageSeries && settingsOpen,
    });

    const currentSeriesId = postQuery.data?.series?.id ?? null;

    const currentSeriesQuery = useQuery({
        queryKey: queryKeys.series.detail(currentSeriesId ?? ''),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/series/{id}', {
                    params: { path: { id: currentSeriesId! } },
                }),
            );
            return res.data;
        },
        enabled: !isCreateMode && canManageSeries && settingsOpen && Boolean(currentSeriesId),
    });

    const currentSeriesItem = (() => {
        const seriesDetail = currentSeriesQuery.data;
        if (!seriesDetail || !postId) return null;
        const rootMatch = seriesDetail.items.find((it) => it.post_id === postId);
        if (rootMatch) {
            return { sectionId: '', displayTitle: rootMatch.display_title ?? '' };
        }
        for (const sec of seriesDetail.sections) {
            const match = sec.items.find((it) => it.post_id === postId);
            if (match) {
                return { sectionId: sec.id, displayTitle: match.display_title ?? '' };
            }
        }
        return null;
    })();

    const seriesItemVersionKey = currentSeriesId
        ? `${currentSeriesId}:${currentSeriesQuery.data?.edit_version ?? 0}`
        : '';
    const [seriesItemDraft, setSeriesItemDraft] = useState<{
        key: string;
        displayTitle: string;
        sectionId: string;
    } | null>(null);

    const activeSeriesDisplayTitle =
        seriesItemDraft?.key === seriesItemVersionKey
            ? seriesItemDraft.displayTitle
            : (currentSeriesItem?.displayTitle ?? '');
    const activeSeriesSectionId =
        seriesItemDraft?.key === seriesItemVersionKey
            ? seriesItemDraft.sectionId
            : (currentSeriesItem?.sectionId ?? '');

    const seriesItemDirty =
        Boolean(currentSeriesItem) &&
        (activeSeriesDisplayTitle.trim() !== currentSeriesItem!.displayTitle.trim() ||
            activeSeriesSectionId !== currentSeriesItem!.sectionId);

    // 更新文章在专栏中的目录短标题与所属篇章
    const updateSeriesItemMutation = useMutation({
        mutationFn: async (input: {
            seriesId: string;
            displayTitle: string;
            targetSectionId: string;
        }) => {
            if (!postId) throw new Error('无效的文章 ID');
            const detailRes = await unwrapResponse(
                apiClient.GET('/api/v1/admin/series/{id}', {
                    params: { path: { id: input.seriesId } },
                }),
            );
            const latest = detailRes.data;
            const normalizedTitle = input.displayTitle.trim() || null;

            const toOutlineItem = (it: Schemas['SeriesItem']) => ({
                post_id: it.post_id,
                display_title: it.post_id === postId ? normalizedTitle : it.display_title,
            });

            const inRoot = latest.items.some((it) => it.post_id === postId);
            const currentSecId = inRoot
                ? ''
                : (latest.sections.find((sec) => sec.items.some((it) => it.post_id === postId))
                      ?.id ?? '');
            const movedGroup = currentSecId !== input.targetSectionId;

            const nextRootItems = movedGroup
                ? latest.items.filter((it) => it.post_id !== postId).map(toOutlineItem)
                : latest.items.map(toOutlineItem);
            if (movedGroup && input.targetSectionId === '') {
                nextRootItems.push({ post_id: postId, display_title: normalizedTitle });
            }

            const nextSections = latest.sections.map((sec) => {
                const baseItems = movedGroup
                    ? sec.items.filter((it) => it.post_id !== postId).map(toOutlineItem)
                    : sec.items.map(toOutlineItem);
                if (movedGroup && sec.id === input.targetSectionId) {
                    baseItems.push({ post_id: postId, display_title: normalizedTitle });
                }
                return {
                    id: sec.id,
                    title: sec.title,
                    description: sec.description,
                    items: baseItems,
                };
            });

            const res = await unwrapResponse(
                apiClient.PUT('/api/v1/admin/series/{id}/outline', {
                    params: { path: { id: input.seriesId } },
                    body: {
                        expected_edit_version: latest.edit_version,
                        items: nextRootItems,
                        sections: nextSections,
                    },
                }),
            );
            return res.data;
        },
        onSuccess: (edited) => {
            setSeriesItemDraft(null);
            queryClient.setQueryData(queryKeys.series.detail(edited.series.id), edited.series);
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            }
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            if (edited.publication) {
            }
            toast(edited.publication ? '专栏目录已更新' : '专栏目录设置已保存', 'ok');
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    // 加入系列末尾（按 §5.3 先读取系列详情获取最新 edit_version）
    const addToSeriesMutation = useMutation({
        mutationFn: async (targetSeriesId: string) => {
            if (!postId) throw new Error('请先保存创建文章');
            const detailRes = await unwrapResponse(
                apiClient.GET('/api/v1/admin/series/{id}', {
                    params: { path: { id: targetSeriesId } },
                }),
            );
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/series/{id}/items', {
                    params: { path: { id: targetSeriesId } },
                    body: {
                        expected_edit_version: detailRes.data.edit_version,
                        post_id: postId,
                    },
                }),
            );
            return { series: detailRes.data, added: res.data };
        },
        onSuccess: ({ series, added }) => {
            setSelectedSeriesId('');
            toast(
                added.publication
                    ? `已加入系列《${series.title}》，公开目录已更新`
                    : `已加入系列《${series.title}》末尾`,
                'ok',
            );
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            }
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(series.id) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
            if (added.publication) {
            }
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    // 从系列移出（按 §5.3 先读取系列详情获取最新 edit_version）
    const removeFromSeriesMutation = useMutation({
        mutationFn: async (targetSeriesId: string) => {
            if (!postId) throw new Error('无效的文章 ID');
            const detailRes = await unwrapResponse(
                apiClient.GET('/api/v1/admin/series/{id}', {
                    params: { path: { id: targetSeriesId } },
                }),
            );
            await unwrapResponse(
                apiClient.DELETE('/api/v1/admin/series/{id}/items/{post_id}', {
                    params: {
                        path: { id: targetSeriesId, post_id: postId },
                        query: { expected_edit_version: detailRes.data.edit_version },
                    },
                }),
            );
            return detailRes.data;
        },
        onSuccess: (series) => {
            toast(`已从系列《${series.title}》移出`, 'ok');
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            }
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.detail(series.id) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.series.all });
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    // 首次加载文章数据时同步到编辑表单
    useEffect(() => {
        const post = postQuery.data;
        if (!post) return;
        if (hydratedPostIdRef.current !== post.id) {
            hydratedPostIdRef.current = post.id;
            setDraft(postToDraft(post));
            setExpectedRevisionId(post.current_revision_id);

            setIsDirty(false);
            setHasConflict(false);
            setSaveState({
                kind: 'saved',
                text: `已保存 · ${formatRelativeTime(post.current_revision.created_at)}`,
            });
        }
    }, [postQuery.data]);

    // 浏览器刷新 / 关闭页签前的防误触提示
    useEffect(() => {
        if (!hasContentToConfirmLeave) return;
        const onBeforeUnload = (event: BeforeUnloadEvent) => {
            if (confirmedLeaveRef.current) return;
            event.preventDefault();
        };
        window.addEventListener('beforeunload', onBeforeUnload);
        return () => window.removeEventListener('beforeunload', onBeforeUnload);
    }, [hasContentToConfirmLeave]);

    // 创建文章 mutation
    const createPostMutation = useMutation({
        mutationFn: async (currentDraft: DraftFields) => {
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/posts', {
                    body: {
                        slug: currentDraft.slug.trim(),
                        ...toRevisionBody(currentDraft),
                    },
                }),
            );
            return res.data;
        },
        onSuccess: (created) => {
            setSettingsOpen(false);
            setSlugError(null);
            queryClient.setQueryData(queryKeys.posts.detail(created.id), created);
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.taxonomy.categories });
            void queryClient.invalidateQueries({ queryKey: queryKeys.taxonomy.tags });
        },
        onError: (error) => {
            const msg = formatErrorWithRequestId(error);
            setSaveState({ kind: 'failed', text: msg });
            const slugDetail = isApiError(error)
                ? error.details.find((item) => item.field === 'slug')
                : undefined;
            const hasSlugFieldError =
                isApiError(error) && (error.code === 'slug_in_use' || Boolean(slugDetail));
            if (hasSlugFieldError) {
                setSlugError(
                    error.code === 'slug_in_use' ? '该 Slug 已被其他文章占用，请更换后再试。' : msg,
                );
                setSettingsOpen(true);
                window.requestAnimationFrame(() => slugInputRef.current?.focus());
            }
            if (!publishing.current) toast(msg, 'bad');
        },
    });

    // 保存新修订（含自动保存）
    const saveRevisionMutation = useMutation({
        mutationFn: async (payload: {
            postId: string;
            expectedId: string;
            draft: DraftFields;
            sequence: number;
        }) => {
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/posts/{id}/revisions', {
                    params: { path: { id: payload.postId } },
                    body: {
                        expected_revision_id: payload.expectedId,
                        ...toRevisionBody(payload.draft),
                    },
                }),
            );
            return res.data;
        },
        onSuccess: (revision, payload) => {
            const editedDuringSave = editSequence.current !== payload.sequence;
            setExpectedRevisionId(revision.id);

            setIsDirty(editedDuringSave);
            setHasConflict(false);
            setSaveState({
                kind: editedDuringSave ? 'dirty' : 'saved',
                text: editedDuringSave ? '有未保存的更改' : '已保存 · 刚刚',
            });
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
                void queryClient.invalidateQueries({ queryKey: queryKeys.taxonomy.categories });
                void queryClient.invalidateQueries({ queryKey: queryKeys.taxonomy.tags });
            }
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'revision_conflict') {
                // 收到 409 revision_conflict 时立即停止自动保存，保留本地输入
                setHasConflict(true);
                setSaveState({
                    kind: 'conflict',
                    text: '有新版本，已停止自动保存',
                });
                return;
            }
            setSaveState({
                kind: 'failed',
                text: `保存失败：${formatErrorWithRequestId(error)}`,
            });
        },
    });

    const { mutateAsync: saveRevision, isPending: saving } = saveRevisionMutation;
    const saveDraftRevision = useCallback(
        (snapshot: DraftFields, expectedId: string, sequence: number) => {
            setSaveState({ kind: 'saving', text: '保存中…' });
            const result = saveRevision({ sequence, postId: postId!, expectedId, draft: snapshot });
            activeSave.current = { sequence, result };
            return result.finally(() => {
                if (activeSave.current?.result === result) activeSave.current = null;
            });
        },
        [postId, saveRevision],
    );

    const triggerSaveRevision = useCallback(
        (snapshot: DraftFields) => {
            if (
                !postId ||
                !expectedRevisionId ||
                hasConflict ||
                activeSave.current ||
                publishing.current
            )
                return;
            if (!snapshot.title.trim()) {
                setSaveState({ kind: 'failed', text: '请先填写文章标题后再保存。' });
                return;
            }
            if (utf8ByteLength(snapshot.markdown_body) > 524_288) {
                setSaveState({ kind: 'failed', text: 'Markdown 正文超出 512 KiB 限制。' });
                return;
            }
            // 保存状态和错误由 mutation 统一反馈。
            void saveDraftRevision(snapshot, expectedRevisionId, editSequence.current).catch(
                () => {},
            );
        },
        [postId, expectedRevisionId, hasConflict, saveDraftRevision],
    );

    useEffect(() => {
        if (isCreateMode || !isDirty || isPublishing || saving || saveState.kind !== 'dirty')
            return;
        const timer = window.setTimeout(() => triggerSaveRevision(draft), 1500);
        return () => window.clearTimeout(timer);
    }, [draft, isCreateMode, isDirty, isPublishing, saving, saveState.kind, triggerSaveRevision]);

    const patchDraft = useCallback(
        (updater: (prev: DraftFields) => DraftFields) => {
            editSequence.current += 1;
            setDraft(updater);
            setIsDirty(true);
            if (!hasConflict) {
                setSaveState({
                    kind: 'dirty',
                    text: isCreateMode ? '草稿未创建' : '有未保存的更改',
                });
            }
        },
        [isCreateMode, hasConflict],
    );

    // 独立修改 Slug（仅在未锁定时）
    const updateSlugMutation = useMutation({
        mutationFn: async (slug: string) => {
            const res = await unwrapResponse(
                apiClient.PATCH('/api/v1/admin/posts/{id}', {
                    params: { path: { id: postId! } },
                    body: { slug },
                }),
            );
            return res.data;
        },
        onSuccess: (updated) => {
            if (!publishing.current) toast('Slug 已更新', 'ok');
            queryClient.setQueryData(queryKeys.posts.detail(updated.id), updated);
            setDraft((prev) => ({ ...prev, slug: updated.slug }));
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    async function refreshPublication(id: string) {
        // 先取消保存草稿触发的旧查询，避免把发布前的线上状态当成读回结果。
        await queryClient.cancelQueries({ queryKey: queryKeys.posts.detail(id) });
        await queryClient.invalidateQueries({ queryKey: queryKeys.posts.all, refetchType: 'none' });
        void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
        return queryClient.fetchQuery({
            queryKey: queryKeys.posts.detail(id),
            queryFn: async () =>
                (
                    await unwrapResponse(
                        apiClient.GET('/api/v1/admin/posts/{id}', {
                            params: { path: { id } },
                        }),
                    )
                ).data,
        });
    }

    // 取消发布
    const unpublishMutation = useMutation({
        mutationFn: async (expectedVersion: number) => {
            const res = await unwrapResponse(
                apiClient.DELETE('/api/v1/admin/posts/{id}/publication', {
                    params: {
                        path: { id: postId! },
                        query: { expected_publication_version: expectedVersion },
                    },
                }),
            );
            return res.data;
        },
        onSuccess: async () => {
            setUnpublishModalOpen(false);
            try {
                await refreshPublication(postId!);
                toast('文章已下线', 'ok');
            } catch {
                toast('文章已下线，请刷新编辑页获取最新状态', 'info');
            }
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'publication_conflict' && postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const openSettingsManually = useCallback(() => {
        setSettingsOpen(true);
    }, []);

    const closeSettingsDrawer = useCallback(() => {
        if (pickerMode !== null) return;
        setSettingsOpen(false);
    }, [pickerMode]);

    const postSeriesId = postQuery.data?.series?.id;

    const navigateBack = useCallback(() => {
        if (canManageSeries) {
            if (postSeriesId) {
                void navigate({
                    to: '/series/$seriesId',
                    params: { seriesId: postSeriesId },
                });
                return;
            }
            void navigate({ to: '/series' });
            return;
        }
        void navigate({ to: '/posts' });
    }, [canManageSeries, navigate, postSeriesId]);

    const handleBackClick = useCallback(() => {
        if (hasContentToConfirmLeave) {
            setLeaveModalOpen(true);
            return;
        }
        navigateBack();
    }, [hasContentToConfirmLeave, navigateBack]);

    const confirmLeave = useCallback(() => {
        confirmedLeaveRef.current = true;
        setLeaveModalOpen(false);
        navigateBack();
    }, [navigateBack]);

    const handleUploadEditorImage = useCallback(
        async (file: File) => {
            if (!canUploadMedia) {
                toast('当前账户没有上传媒体的权限', 'warn');
                return null;
            }
            if (file.type && !ALLOWED_IMAGE_MIME_TYPES.has(file.type.toLowerCase())) {
                toast('仅支持上传 JPEG、PNG、WebP 格式的图片', 'warn');
                return null;
            }
            if (file.size > MAX_IMAGE_UPLOAD_BYTES) {
                toast('图片体积超过 10 MiB 限制', 'warn');
                return null;
            }
            setUploadingImages((count) => count + 1);
            try {
                const normalized = normalizePastedImageFile(file);
                const uploaded = await uploadMediaFile(normalized);
                void queryClient.invalidateQueries({ queryKey: queryKeys.media.all });
                if (uploaded.media.status !== 'ready') {
                    toast(
                        uploaded.media.rejection_reason
                            ? getMediaRejectionMessage(uploaded.media.rejection_reason)
                            : '图片未通过服务端校验，无法插入',
                        'bad',
                    );
                    return null;
                }
                const alt = uploaded.media.alt_text?.trim() ?? '';
                toast(`已上传并插入图片：${uploaded.media.original_filename}`, 'ok');
                return { src: `media:${uploaded.media.id}`, alt };
            } catch (error) {
                toast(`图片上传失败：${formatErrorWithRequestId(error)}`, 'bad');
                return null;
            } finally {
                setUploadingImages((count) => count - 1);
            }
        },
        [canUploadMedia, queryClient, toast],
    );

    function prepareDraft(): DraftFields | null {
        if (uploadingImages > 0) {
            toast('正文图片正在上传，完成后即可继续', 'info');
            return null;
        }
        if (!draft.title.trim()) {
            setSettingsOpen(false);
            setMobileTab('source');
            toast('请先输入文章标题', 'warn');
            window.requestAnimationFrame(() => titleInputRef.current?.focus());
            return null;
        }
        if (utf8ByteLength(draft.markdown_body) > 524_288) {
            toast('Markdown 正文超出 512 KiB 限制', 'warn');
            return null;
        }
        if (isCreateMode && !draft.slug.trim()) {
            const prefix =
                draft.title
                    .toLowerCase()
                    .replace(/[^a-z0-9]+/g, '-')
                    .slice(0, 70)
                    .replace(/^-+|-+$/g, '') || 'post';
            const suffix = Array.from(crypto.getRandomValues(new Uint8Array(6)), (byte) =>
                byte.toString(16).padStart(2, '0'),
            ).join('');
            const slug = `${prefix}-${suffix}`;
            setDraft((previous) => ({ ...previous, slug }));
            return { ...draft, slug };
        }
        if (!isValidSlug(draft.slug)) {
            setSlugError(
                'Slug 仅允许小写字母、数字和单个连字符，不能以连字符开头或结尾（1–100 字符）',
            );
            setSettingsOpen(true);
            window.requestAnimationFrame(() => slugInputRef.current?.focus());
            return null;
        }
        setSlugError(null);
        return draft;
    }

    function openCreatedPost(id: string) {
        confirmedLeaveRef.current = true;
        void navigate({ to: '/posts/$postId/edit', params: { postId: id } });
    }

    function handleCreatePost() {
        const snapshot = prepareDraft();
        if (!snapshot || publishing.current || createPostMutation.isPending) return;
        createPostMutation.mutate(snapshot, {
            onSuccess: (created) => {
                toast('草稿已保存', 'ok');
                openCreatedPost(created.id);
            },
        });
    }

    async function handlePublish() {
        if (!canPublish || publishing.current || hasConflict || createPostMutation.isPending)
            return;
        const snapshot = prepareDraft();
        if (!snapshot) return;
        const sequence = editSequence.current;
        let created: Schemas['Post'] | undefined;
        publishing.current = true;
        setIsPublishing(true);
        try {
            if (isCreateMode) created = await createPostMutation.mutateAsync(snapshot);
            const article = created ?? postQuery.data;
            if (!article) return;
            let revisionId = created?.current_revision_id ?? expectedRevisionId;
            if (!revisionId) return;
            const pending = activeSave.current;
            if (pending) revisionId = (await pending.result).id;
            if (!created && isDirty && pending?.sequence !== sequence) {
                revisionId = (await saveDraftRevision(snapshot, revisionId, sequence)).id;
            }
            if (!article.slug_locked && snapshot.slug.trim() !== article.slug) {
                await updateSlugMutation.mutateAsync(snapshot.slug.trim());
            }
            await unwrapResponse(
                apiClient.PUT('/api/v1/admin/posts/{id}/publication', {
                    params: { path: { id: article.id } },
                    body: {
                        revision_id: revisionId,
                        expected_publication_version: article.publication.publication_version,
                    },
                }),
            );
            try {
                await refreshPublication(article.id);
                toast(
                    editSequence.current === sequence
                        ? '文章已上线'
                        : '点击发布时的版本已上线，后续编辑仍在草稿中。',
                    'ok',
                );
            } catch {
                toast('文章已上线，请刷新编辑页获取最新状态', 'info');
            }
        } catch (error) {
            // 协作者可能在保存成功、开始发布之间替换唯一的当前修订。
            if (
                isApiError(error) &&
                error.code === 'validation_error' &&
                error.details.some((detail) => detail.field === 'revision_id')
            ) {
                setHasConflict(true);
                setSaveState({ kind: 'conflict', text: '有新版本，已停止自动保存' });
            }
            if (postId)
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            toast(formatErrorWithRequestId(error), 'bad');
        } finally {
            publishing.current = false;
            setIsPublishing(false);
            if (created) openCreatedPost(created.id);
        }
    }

    const post = postQuery.data;
    const previewHtml = useMemo(
        () => renderSafeMarkdown(draft.markdown_body),
        [draft.markdown_body],
    );

    const creating = isCreateMode && (isPublishing || createPostMutation.isPending);

    if (!isCreateMode && postQuery.isLoading) {
        return <LoadingCard text="正在加载文章内容…" />;
    }

    if (!isCreateMode && postQuery.isError && !post) {
        return (
            <ErrorBanner
                title="无法读取文章"
                error={postQuery.error}
                onRetry={() => void postQuery.refetch()}
            />
        );
    }

    const publishedRevId = post?.publication.published_revision_id ?? null;

    const alreadyLive =
        !isDirty && Boolean(publishedRevId) && publishedRevId === expectedRevisionId;
    const canUnpublishNow = !isCreateMode && Boolean(post?.publication.published_revision_id);

    const reloadLatestAfterConflict = async () => {
        if (!postId) return;
        const result = await postQuery.refetch();
        if (result.data) {
            setDraft(postToDraft(result.data));
            setExpectedRevisionId(result.data.current_revision_id);

            setIsDirty(false);
            setHasConflict(false);
            setSaveState({
                kind: 'saved',
                text: `已读取最新修订 r${result.data.current_revision.revision_no}`,
            });
            toast(`已加载最新修订 r${result.data.current_revision.revision_no}`, 'info');
        }
    };

    const categories = categoriesQuery.data?.data ?? [];
    const tags = tagsQuery.data?.data ?? [];
    const backLabel = canManageSeries
        ? postSeriesId
            ? '返回专栏工作台'
            : '返回专栏列表'
        : '返回文章列表';

    const publication = postPublicationStatus({
        current_revision_id: isDirty ? null : expectedRevisionId,
        published_revision_id: publishedRevId,
    });

    return (
        <div className="space-y-3" inert={creating} aria-busy={isPublishing || creating}>
            {/* 独立返回导航 */}
            <div>
                <button
                    type="button"
                    onClick={handleBackClick}
                    aria-label="返回文章列表"
                    className="-ml-2 inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[13px] font-medium text-ui-sub transition-colors hover:bg-white hover:text-ui-text"
                >
                    <ArrowLeft className="h-3.5 w-3.5" />
                    <span>{backLabel}</span>
                </button>
            </div>

            {/* 顶栏操作区：整合保存状态、发布状态与全局操作 */}
            <header
                aria-label="文章操作栏"
                className="flex flex-wrap items-center justify-between gap-2.5 rounded-xl border border-ui-line bg-white px-3.5 py-2.5 shadow-[var(--shadow-card)] sm:px-4"
            >
                <div className="flex min-w-0 flex-wrap items-center gap-2.5">
                    <div
                        className="flex min-w-0 items-center gap-2 text-[12.5px]"
                        aria-live="polite"
                    >
                        <span
                            className={cn(
                                'inline-flex items-center gap-1.5 truncate',
                                saveState.kind === 'saved' && 'text-ok',
                                (saveState.kind === 'dirty' || saveState.kind === 'conflict') &&
                                    'text-warn',
                                saveState.kind === 'failed' && 'text-bad',
                                (saveState.kind === 'idle' || saveState.kind === 'saving') &&
                                    'text-ui-sub',
                            )}
                        >
                            <span
                                className={cn(
                                    'h-1.5 w-1.5 shrink-0 rounded-full',
                                    saveState.kind === 'saved' && 'bg-ok',
                                    (saveState.kind === 'dirty' || saveState.kind === 'conflict') &&
                                        'bg-warn',
                                    saveState.kind === 'failed' && 'bg-bad',
                                    (saveState.kind === 'idle' || saveState.kind === 'saving') &&
                                        'bg-ui-sub/50',
                                )}
                                aria-hidden="true"
                            />
                            <span className="truncate">{saveState.text}</span>
                        </span>
                        {saveState.kind === 'failed' && !isCreateMode ? (
                            <button
                                type="button"
                                onClick={() => triggerSaveRevision(draft)}
                                className="shrink-0 font-medium text-accent hover:underline"
                            >
                                重试保存
                            </button>
                        ) : null}
                    </div>

                    {!isCreateMode ? (
                        <>
                            <span
                                className="hidden h-4 w-px shrink-0 bg-ui-line sm:inline-block"
                                aria-hidden="true"
                            />
                            <div
                                aria-label="发布状态"
                                className="flex flex-wrap items-center gap-1.5"
                            >
                                <StatusChip tone={publication.tone}>{publication.label}</StatusChip>
                                {publishedRevId && post && (
                                    <a
                                        className="text-sm text-accent hover:underline"
                                        href={new URL(`/blog/${post.slug}/`, websiteUrl).href}
                                        target="_blank"
                                        rel="noreferrer"
                                    >
                                        查看线上文章
                                    </a>
                                )}
                            </div>
                        </>
                    ) : null}
                </div>

                <div className="flex flex-wrap items-center gap-2">
                    {/* 桌面端模式切换：默认专注模式 */}
                    <fieldset
                        aria-label="编辑视图模式"
                        className="hidden items-center rounded-lg border border-ui-line bg-ui-subtle p-0.5 lg:inline-flex"
                    >
                        <button
                            type="button"
                            onClick={() => setPreviewOpen(false)}
                            aria-pressed={!previewOpen}
                            className={cn(
                                'inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] font-medium whitespace-nowrap transition-colors',
                                !previewOpen
                                    ? 'bg-white text-ui-text shadow-2xs'
                                    : 'text-ui-sub hover:text-ui-text',
                            )}
                        >
                            <PenLine className="h-3.5 w-3.5" />
                            专注
                        </button>
                        <button
                            type="button"
                            onClick={() => setPreviewOpen(true)}
                            aria-pressed={previewOpen}
                            className={cn(
                                'inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] font-medium whitespace-nowrap transition-colors',
                                previewOpen
                                    ? 'bg-white text-ui-text shadow-2xs'
                                    : 'text-ui-sub hover:text-ui-text',
                            )}
                        >
                            <Columns2 className="h-3.5 w-3.5" />
                            预览
                        </button>
                    </fieldset>

                    {/* 文章设置抽屉入口 */}
                    <Button
                        variant="secondary"
                        size="sm"
                        aria-expanded={settingsOpen}
                        aria-controls="post-settings-drawer"
                        onClick={openSettingsManually}
                        className="relative"
                    >
                        <SlidersHorizontal className="h-3.5 w-3.5 text-ui-sub" />
                        <span>文章设置</span>
                    </Button>

                    {isCreateMode ? (
                        <Button
                            variant={canPublish ? 'secondary' : 'primary'}
                            size="sm"
                            disabled={createPostMutation.isPending || isPublishing}
                            onClick={handleCreatePost}
                        >
                            {createPostMutation.isPending && !isPublishing ? '保存中…' : '保存草稿'}
                        </Button>
                    ) : (
                        <Button
                            variant="secondary"
                            size="sm"
                            disabled={!isDirty || hasConflict || saving || isPublishing}
                            onClick={() => triggerSaveRevision(draft)}
                        >
                            保存文章
                        </Button>
                    )}
                    {canPublish && (
                        <>
                            {canUnpublishNow && (
                                <Button
                                    variant="danger-outline"
                                    size="sm"
                                    disabled={isPublishing || unpublishMutation.isPending}
                                    onClick={() => setUnpublishModalOpen(true)}
                                >
                                    取消发布
                                </Button>
                            )}
                            <Button
                                variant="primary"
                                size="sm"
                                disabled={
                                    isPublishing ||
                                    unpublishMutation.isPending ||
                                    createPostMutation.isPending ||
                                    updateSlugMutation.isPending ||
                                    hasConflict ||
                                    alreadyLive ||
                                    (!isCreateMode && !expectedRevisionId)
                                }
                                onClick={() => void handlePublish()}
                            >
                                {isPublishing
                                    ? '发布中…'
                                    : publishedRevId
                                      ? '发布更新'
                                      : '发布文章'}
                            </Button>
                        </>
                    )}
                </div>
            </header>

            {/* 409 修订冲突横幅：停止自动保存并保留用户输入 */}
            {hasConflict ? (
                <div
                    role="alert"
                    className="rounded-lg border border-[#EBD7A9] bg-warn-tint px-4 py-3 text-[14px] text-warn"
                >
                    <div className="flex flex-wrap items-center justify-between gap-3">
                        <p>
                            <strong className="font-medium">有新版本，已停止自动保存。</strong>
                            服务端已存在更新的修订，你的编辑内容仍完整保留在下方，不会被静默覆盖。
                        </p>
                        <div className="flex shrink-0 gap-2">
                            <Button
                                variant="secondary"
                                size="sm"
                                onClick={async () => {
                                    await navigator.clipboard?.writeText(draft.markdown_body);
                                    toast('已将你的 Markdown 正文复制到剪贴板', 'ok');
                                }}
                            >
                                复制我的内容
                            </Button>
                            <Button
                                variant="primary"
                                size="sm"
                                onClick={() => void reloadLatestAfterConflict()}
                            >
                                读取最新修订
                            </Button>
                        </div>
                    </div>
                </div>
            ) : null}

            {/* 窄屏视图切换：编辑 / 预览（文章设置由顶栏抽屉统一打开） */}
            <div
                role="tablist"
                aria-label="编辑器视图"
                className="grid grid-cols-2 rounded-lg border border-ui-line bg-white p-1 text-[14px] lg:hidden"
            >
                {(
                    [
                        ['source', '编辑'],
                        ['preview', '预览'],
                    ] as const
                ).map(([key, label]) => (
                    <button
                        key={key}
                        type="button"
                        role="tab"
                        aria-selected={mobileTab === key}
                        onClick={() => setMobileTab(key)}
                        className="h-9 rounded-md aria-selected:bg-accent-tint aria-selected:font-medium aria-selected:text-accent"
                    >
                        {label}
                    </button>
                ))}
            </div>

            {/* 主体写作区：默认专注单栏，切换预览后双栏对照 */}
            <div className={cn('grid gap-3', previewOpen ? 'lg:grid-cols-2' : 'w-full')}>
                {/* 文章标题 + Tiptap Markdown 编辑器 */}
                <section
                    aria-label="正文编辑器"
                    className={cn(
                        'studio-card flex min-h-[500px] flex-col overflow-hidden lg:h-[calc(100vh-11.5rem)] lg:min-h-[560px]',
                        mobileTab !== 'source' && 'max-lg:hidden',
                    )}
                >
                    {/* 独立的文章标题输入区 */}
                    <div className="shrink-0 border-b border-ui-line/80 bg-white px-5 pt-4 pb-3.5 sm:px-6">
                        <label htmlFor="post-title" className="sr-only">
                            文章标题
                        </label>
                        <input
                            ref={titleInputRef}
                            id="post-title"
                            type="text"
                            value={draft.title}
                            maxLength={200}
                            onChange={(event) =>
                                patchDraft((prev) => ({ ...prev, title: event.target.value }))
                            }
                            placeholder="输入文章标题…"
                            className="block w-full border-0 bg-transparent p-0 text-[22px] leading-snug font-semibold tracking-[-0.02em] text-ui-text placeholder:text-[#98ABA8] focus:ring-0 focus:outline-none sm:text-[25px]"
                        />
                    </div>

                    <TiptapMarkdownEditor
                        ref={editorRef}
                        value={draft.markdown_body}
                        readOnly={creating}
                        onChange={(nextValue) =>
                            patchDraft((prev) => ({ ...prev, markdown_body: nextValue }))
                        }
                        onRequestInsertMedia={() => setPickerMode('body')}
                        onUploadImage={handleUploadEditorImage}
                    />

                    <div className="flex h-8 shrink-0 items-center justify-between border-t border-ui-line bg-ui-subtle/60 px-4 text-[11.5px] text-ui-sub">
                        <span className="tnum font-code">
                            {draft.markdown_body.length} 字符 ·{' '}
                            {(utf8ByteLength(draft.markdown_body) / 1024).toFixed(1)} KiB
                        </span>
                        <span>{isCreateMode ? '创建后开启自动保存' : '自动保存间隔 1.5 秒'}</span>
                    </div>
                </section>

                {/* 安全预览（仅在开启预览模式或窄屏切到预览标签时显示） */}
                <section
                    aria-label="预览"
                    className={cn(
                        'studio-card flex min-h-[500px] flex-col overflow-hidden lg:h-[calc(100vh-11.5rem)] lg:min-h-[560px]',
                        mobileTab !== 'preview' && 'max-lg:hidden',
                        !previewOpen && 'lg:hidden',
                    )}
                >
                    <div className="flex h-11 shrink-0 items-center justify-between border-b border-ui-line bg-ui-subtle/80 px-4 text-[12.5px] text-ui-sub">
                        <span className="font-medium text-ui-text">安全预览</span>
                        <span>仅后台可见，不生成公开地址</span>
                    </div>
                    <div className="flex-1 overflow-y-auto bg-paper px-6 py-6">
                        <h1 className="text-[26px] leading-tight font-semibold tracking-[-0.02em] text-ink">
                            {draft.title.trim() || '（未填写标题）'}
                        </h1>
                        {draft.excerpt.trim() ? (
                            <p className="mt-2.5 text-[14px] leading-relaxed text-ink-soft">
                                {draft.excerpt}
                            </p>
                        ) : null}
                        <div className="mt-5">
                            <SafeHtmlPreview
                                html={previewHtml}
                                emptyText="正文预览会显示在这里。原始 HTML 按文本显示，图片使用 ![说明](media:<ID>)。"
                            />
                        </div>
                    </div>
                </section>
            </div>

            {/* 文章设置抽屉 */}
            <Drawer
                open={settingsOpen}
                onClose={closeSettingsDrawer}
                title="文章设置"
                subtitle="文章发布设置"
                widthClassName="sm:w-[440px]"
                footer={
                    isCreateMode ? (
                        <div className="flex items-center justify-end gap-2">
                            <Button variant="secondary" onClick={closeSettingsDrawer}>
                                稍后设置
                            </Button>
                            <Button
                                variant="primary"
                                disabled={createPostMutation.isPending || isPublishing}
                                onClick={handleCreatePost}
                            >
                                {createPostMutation.isPending ? '保存中…' : '保存草稿'}
                            </Button>
                        </div>
                    ) : (
                        <div className="flex items-center justify-between gap-2">
                            <span className="text-[12px] text-ui-sub">修改后自动保存</span>
                            <Button variant="secondary" onClick={closeSettingsDrawer}>
                                完成
                            </Button>
                        </div>
                    )
                }
            >
                <div id="post-settings-drawer" inert={creating} className="space-y-5 text-[14px]">
                    {/* Slug */}
                    <div>
                        <label htmlFor="post-slug" className="block font-medium text-ui-text">
                            Slug
                        </label>
                        <div className="mt-1.5 flex gap-1.5">
                            <input
                                ref={slugInputRef}
                                id="post-slug"
                                type="text"
                                value={draft.slug}
                                readOnly={Boolean(post?.slug_locked)}
                                aria-invalid={Boolean(slugError)}
                                aria-describedby={slugError ? 'post-slug-error' : undefined}
                                onChange={(event) => {
                                    const val = event.target.value;
                                    setSlugError(null);
                                    setDraft((prev) => ({ ...prev, slug: val }));
                                }}
                                placeholder="immutable-revision-flow"
                                className={cn(
                                    'studio-input min-w-0 flex-1 font-code text-[13px]',
                                    slugError &&
                                        'border-bad focus:border-bad focus:shadow-[0_0_0_3px_rgba(180,35,24,0.14)]',
                                )}
                            />
                            {!isCreateMode && !post?.slug_locked ? (
                                <Button
                                    variant="secondary"
                                    size="sm"
                                    disabled={
                                        updateSlugMutation.isPending ||
                                        draft.slug.trim() === post?.slug
                                    }
                                    onClick={() => updateSlugMutation.mutate(draft.slug.trim())}
                                >
                                    更新
                                </Button>
                            ) : null}
                        </div>
                        {slugError ? (
                            <p
                                id="post-slug-error"
                                role="alert"
                                className="mt-1.5 text-[12px] text-bad"
                            >
                                {slugError}
                            </p>
                        ) : null}
                        <p className="mt-1 text-[12px] text-ui-sub">
                            {post?.slug_locked
                                ? '首次发布后已锁定，一期不支持改名。'
                                : '新文章留空会自动生成。可使用小写字母、数字和连字符，首次发布后锁定。'}
                        </p>
                    </div>

                    {/* 摘要 */}
                    <div>
                        <div className="flex justify-between">
                            <label
                                htmlFor="post-excerpt"
                                className="block font-medium text-ui-text"
                            >
                                摘要
                            </label>
                            <span className="tnum text-[12px] text-ui-sub">
                                {draft.excerpt.length}/500
                            </span>
                        </div>
                        <textarea
                            id="post-excerpt"
                            rows={3}
                            maxLength={500}
                            value={draft.excerpt}
                            onChange={(event) =>
                                patchDraft((prev) => ({
                                    ...prev,
                                    excerpt: event.target.value,
                                }))
                            }
                            placeholder="文章列表卡片显示的简短摘要（可选）"
                            className="studio-input mt-1.5 h-auto py-2 text-[13px]"
                        />
                    </div>

                    {/* 分类（最多 5 个） */}
                    <fieldset>
                        <legend className="font-medium text-ui-text">
                            分类{' '}
                            <span className="text-[12px] font-normal text-ui-sub">
                                （{draft.category_ids.length}/5）
                            </span>
                        </legend>
                        {categories.length === 0 ? (
                            <p className="mt-1.5 text-[12px] text-ui-sub">暂无可用分类</p>
                        ) : (
                            <div className="mt-2 flex flex-wrap gap-1.5">
                                {categories.map((cat) => {
                                    const checked = draft.category_ids.includes(cat.id);
                                    return (
                                        <button
                                            key={cat.id}
                                            type="button"
                                            aria-pressed={checked}
                                            onClick={() =>
                                                patchDraft((prev) => {
                                                    if (checked) {
                                                        return {
                                                            ...prev,
                                                            category_ids: prev.category_ids.filter(
                                                                (id) => id !== cat.id,
                                                            ),
                                                        };
                                                    }
                                                    if (prev.category_ids.length >= 5) {
                                                        toast('每篇文章最多选择 5 个分类', 'warn');
                                                        return prev;
                                                    }
                                                    return {
                                                        ...prev,
                                                        category_ids: [
                                                            ...prev.category_ids,
                                                            cat.id,
                                                        ],
                                                    };
                                                })
                                            }
                                            className={cn(
                                                'h-7 rounded-md border px-2.5 text-[12px] transition-colors',
                                                checked
                                                    ? 'border-accent bg-accent-tint font-medium text-accent'
                                                    : 'border-ui-line bg-white text-ui-text hover:bg-ui-bg',
                                            )}
                                        >
                                            {cat.name}
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                    </fieldset>

                    {/* 标签（最多 20 个） */}
                    <fieldset>
                        <legend className="font-medium text-ui-text">
                            标签{' '}
                            <span className="text-[12px] font-normal text-ui-sub">
                                （{draft.tag_ids.length}/20）
                            </span>
                        </legend>
                        {tags.length === 0 ? (
                            <p className="mt-1.5 text-[12px] text-ui-sub">暂无可用标签</p>
                        ) : (
                            <div className="mt-2 flex flex-wrap gap-1.5">
                                {tags.map((tag) => {
                                    const checked = draft.tag_ids.includes(tag.id);
                                    return (
                                        <button
                                            key={tag.id}
                                            type="button"
                                            aria-pressed={checked}
                                            onClick={() =>
                                                patchDraft((prev) => {
                                                    if (checked) {
                                                        return {
                                                            ...prev,
                                                            tag_ids: prev.tag_ids.filter(
                                                                (id) => id !== tag.id,
                                                            ),
                                                        };
                                                    }
                                                    if (prev.tag_ids.length >= 20) {
                                                        toast('每篇文章最多选择 20 个标签', 'warn');
                                                        return prev;
                                                    }
                                                    return {
                                                        ...prev,
                                                        tag_ids: [...prev.tag_ids, tag.id],
                                                    };
                                                })
                                            }
                                            className={cn(
                                                'h-7 rounded-md border px-2.5 text-[12px] transition-colors',
                                                checked
                                                    ? 'border-accent bg-accent-tint font-medium text-accent'
                                                    : 'border-ui-line bg-white text-ui-text hover:bg-ui-bg',
                                            )}
                                        >
                                            #{tag.name}
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                    </fieldset>

                    {/* 所属专栏（仅对已创建文章且具备 series.manage 权限时显示） */}
                    {!isCreateMode && canManageSeries ? (
                        <section
                            aria-labelledby="post-series-card-h"
                            className="rounded-xl border border-ui-line bg-ui-subtle/50 p-3.5"
                        >
                            <div className="flex items-center gap-1.5">
                                <BookOpen className="h-4 w-4 text-accent" aria-hidden="true" />
                                <h3 id="post-series-card-h" className="font-medium text-ui-text">
                                    所属专栏
                                </h3>
                            </div>
                            {post?.series ? (
                                <div className="mt-2.5 space-y-3 text-[13px]">
                                    <div className="rounded-lg border border-ui-line bg-white px-3 py-2">
                                        <div className="font-medium text-ui-text">
                                            {post.series.title}
                                        </div>
                                        <div className="mt-0.5 text-[12px] text-ui-sub">
                                            {post.series.section_title
                                                ? `篇章：${post.series.section_title} · 组内第 ${post.series.position + 1} 项`
                                                : `根目录 · 第 ${post.series.position + 1} 项`}
                                        </div>
                                    </div>

                                    {currentSeriesQuery.data ? (
                                        <div className="space-y-2.5">
                                            <div>
                                                <label
                                                    htmlFor="post-series-display-title"
                                                    className="block text-[12.5px] font-medium text-ui-text"
                                                >
                                                    目录短标题
                                                </label>
                                                <input
                                                    id="post-series-display-title"
                                                    type="text"
                                                    maxLength={200}
                                                    value={activeSeriesDisplayTitle}
                                                    onChange={(event) =>
                                                        setSeriesItemDraft({
                                                            key: seriesItemVersionKey,
                                                            displayTitle: event.target.value,
                                                            sectionId: activeSeriesSectionId,
                                                        })
                                                    }
                                                    placeholder={`留空则显示「${draft.title || post.current_revision.title}」`}
                                                    className="studio-input mt-1 h-8 text-[12.5px]"
                                                />
                                            </div>
                                            <div>
                                                <label
                                                    htmlFor="post-series-section"
                                                    className="block text-[12.5px] font-medium text-ui-text"
                                                >
                                                    所属篇章
                                                </label>
                                                <div className="mt-1">
                                                    <Select
                                                        id="post-series-section"
                                                        ariaLabel="所属篇章"
                                                        className="w-full"
                                                        value={activeSeriesSectionId}
                                                        onChange={(nextSectionId) =>
                                                            setSeriesItemDraft({
                                                                key: seriesItemVersionKey,
                                                                displayTitle:
                                                                    activeSeriesDisplayTitle,
                                                                sectionId: nextSectionId,
                                                            })
                                                        }
                                                        options={[
                                                            {
                                                                value: '',
                                                                label: '根目录（专栏顶部）',
                                                            },
                                                            ...currentSeriesQuery.data.sections.map(
                                                                (sec, idx) => ({
                                                                    value: sec.id,
                                                                    label: `篇章 ${idx + 1}：${sec.title}`,
                                                                }),
                                                            ),
                                                        ]}
                                                    />
                                                </div>
                                            </div>
                                        </div>
                                    ) : null}

                                    <div className="flex flex-wrap items-center gap-2">
                                        {seriesItemDirty ? (
                                            <Button
                                                variant="primary"
                                                size="sm"
                                                disabled={updateSeriesItemMutation.isPending}
                                                onClick={() => {
                                                    if (!post.series) return;
                                                    updateSeriesItemMutation.mutate({
                                                        seriesId: post.series.id,
                                                        displayTitle: activeSeriesDisplayTitle,
                                                        targetSectionId: activeSeriesSectionId,
                                                    });
                                                }}
                                            >
                                                {updateSeriesItemMutation.isPending
                                                    ? '保存中…'
                                                    : '保存专栏设置'}
                                            </Button>
                                        ) : null}
                                        <Link
                                            to="/series/$seriesId"
                                            params={{ seriesId: post.series.id }}
                                            className="inline-flex h-8 items-center gap-1 rounded-lg border border-ui-line bg-white px-2.5 text-[12.5px] font-medium text-accent transition-colors hover:border-accent/40 hover:bg-accent-tint"
                                        >
                                            前往专栏目录
                                            <ExternalLink className="h-3 w-3" />
                                        </Link>
                                        <Button
                                            variant="danger-outline"
                                            size="sm"
                                            disabled={removeFromSeriesMutation.isPending}
                                            onClick={() => {
                                                if (post.series) {
                                                    removeFromSeriesMutation.mutate(post.series.id);
                                                }
                                            }}
                                        >
                                            {removeFromSeriesMutation.isPending
                                                ? '移出中…'
                                                : '移出专栏'}
                                        </Button>
                                    </div>
                                </div>
                            ) : (
                                <div className="mt-2.5 space-y-2 text-[13px]">
                                    <p className="text-[12px] text-ui-sub">
                                        当前未归属任何专栏，可选择一个专栏并一键追加到目录末尾。
                                    </p>
                                    <div className="flex items-center gap-2">
                                        <div className="min-w-0 flex-1">
                                            <Select
                                                id="post-assign-series"
                                                ariaLabel="选择专栏"
                                                className="w-full"
                                                value={selectedSeriesId}
                                                onChange={setSelectedSeriesId}
                                                options={[
                                                    { value: '', label: '选择目标专栏…' },
                                                    ...(seriesOptionsQuery.data?.data ?? []).map(
                                                        (item) => ({
                                                            value: item.id,
                                                            label: item.title,
                                                        }),
                                                    ),
                                                ]}
                                            />
                                        </div>
                                        <Button
                                            variant="secondary"
                                            size="sm"
                                            disabled={
                                                !selectedSeriesId || addToSeriesMutation.isPending
                                            }
                                            onClick={() => {
                                                if (selectedSeriesId) {
                                                    addToSeriesMutation.mutate(selectedSeriesId);
                                                }
                                            }}
                                        >
                                            {addToSeriesMutation.isPending ? '加入中…' : '加入专栏'}
                                        </Button>
                                    </div>
                                </div>
                            )}
                        </section>
                    ) : null}

                    {/* 封面 */}
                    <div>
                        <span className="block font-medium text-ui-text">封面</span>
                        <div className="mt-1.5 flex items-center gap-3 rounded-md border border-dashed border-ui-line p-2.5">
                            <div className="h-12 w-20 shrink-0 overflow-hidden rounded border border-ui-line bg-[#EEF0F3]">
                                <AuthenticatedImage
                                    mediaId={draft.cover_media_id}
                                    alt="文章封面"
                                    className="h-full w-full object-cover"
                                />
                            </div>
                            <div className="min-w-0 flex-1">
                                <div className="truncate font-code text-[12px] text-ui-sub">
                                    {draft.cover_media_id ?? '未设置封面'}
                                </div>
                                <div className="mt-1 flex gap-3 text-[13px]">
                                    <button
                                        type="button"
                                        onClick={() => setPickerMode('cover')}
                                        className="text-accent hover:underline"
                                    >
                                        {draft.cover_media_id ? '更换' : '选择媒体'}
                                    </button>
                                    {draft.cover_media_id ? (
                                        <button
                                            type="button"
                                            onClick={() =>
                                                patchDraft((prev) => ({
                                                    ...prev,
                                                    cover_media_id: null,
                                                }))
                                            }
                                            className="text-ui-sub hover:text-bad"
                                        >
                                            移除
                                        </button>
                                    ) : null}
                                </div>
                            </div>
                        </div>
                    </div>

                    {/* 首页精选 */}
                    <label
                        htmlFor="post-featured"
                        aria-label="首页精选"
                        className="flex items-start gap-2.5"
                    >
                        <input
                            id="post-featured"
                            type="checkbox"
                            checked={draft.is_featured}
                            onChange={(event) =>
                                patchDraft((prev) => ({
                                    ...prev,
                                    is_featured: event.target.checked,
                                }))
                            }
                            className="mt-0.5 h-4 w-4 accent-[#137A4D]"
                        />
                        <span>
                            <span className="font-medium text-ui-text">首页精选</span>
                            <span className="block text-[12px] text-ui-sub">
                                多篇精选时首页取最新的一篇。
                            </span>
                        </span>
                    </label>

                    {/* SEO */}
                    <fieldset className="space-y-3 border-t border-ui-line pt-4">
                        <legend className="font-medium text-ui-text">SEO</legend>
                        <div>
                            <label
                                htmlFor="post-seo-title"
                                className="block text-[13px] text-ui-sub"
                            >
                                标题（留空则用文章标题）
                            </label>
                            <input
                                id="post-seo-title"
                                type="text"
                                maxLength={200}
                                value={draft.seo_title}
                                onChange={(event) =>
                                    patchDraft((prev) => ({
                                        ...prev,
                                        seo_title: event.target.value,
                                    }))
                                }
                                placeholder={draft.title || '文章标题'}
                                className="studio-input mt-1 text-[13px]"
                            />
                        </div>
                        <div>
                            <div className="flex justify-between">
                                <label
                                    htmlFor="post-seo-desc"
                                    className="block text-[13px] text-ui-sub"
                                >
                                    描述
                                </label>
                                <span className="tnum text-[12px] text-ui-sub">
                                    {draft.seo_description.length}/320
                                </span>
                            </div>
                            <textarea
                                id="post-seo-desc"
                                rows={3}
                                maxLength={320}
                                value={draft.seo_description}
                                onChange={(event) =>
                                    patchDraft((prev) => ({
                                        ...prev,
                                        seo_description: event.target.value,
                                    }))
                                }
                                className="studio-input mt-1 h-auto py-2 text-[13px]"
                            />
                        </div>
                    </fieldset>
                </div>
            </Drawer>

            {/* 返回确认弹窗 */}
            <Modal
                open={leaveModalOpen}
                onClose={() => setLeaveModalOpen(false)}
                role="alertdialog"
                title={isCreateMode ? '放弃当前草稿并返回？' : '有未保存的更改，确定返回？'}
                description={
                    isCreateMode
                        ? '当前已填写文章标题或正文内容，尚未创建保存。返回文章列表后已输入的内容将会丢失。'
                        : '当前文章存在尚未保存的修改，直接返回可能会丢失最近的编辑内容。'
                }
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setLeaveModalOpen(false)}>
                            继续编辑
                        </Button>
                        <Button variant="danger" onClick={confirmLeave}>
                            {isCreateMode ? '放弃并返回' : '仍要返回'}
                        </Button>
                    </>
                }
            >
                <div className="rounded-lg bg-ui-subtle px-3.5 py-2.5 text-[13px] text-ui-sub">
                    {draft.title.trim() ? (
                        <p className="truncate">
                            标题：<span className="font-medium text-ui-text">{draft.title}</span>
                        </p>
                    ) : null}
                    {draft.markdown_body.trim() ? (
                        <p className={cn(draft.title.trim() && 'mt-1')}>
                            正文：已输入{' '}
                            <span className="tnum font-medium text-ui-text">
                                {draft.markdown_body.length}
                            </span>{' '}
                            个字符
                        </p>
                    ) : null}
                </div>
            </Modal>

            {/* 媒体选择器 */}
            <MediaPickerDrawer
                open={pickerMode !== null}
                onClose={() => setPickerMode(null)}
                title={pickerMode === 'cover' ? '选择文章封面' : '插入媒体到正文'}
                confirmLabel={pickerMode === 'cover' ? '设为封面' : '插入正文'}
                onSelect={(media) => {
                    if (pickerMode === 'cover') {
                        patchDraft((prev) => ({ ...prev, cover_media_id: media.id }));
                    } else {
                        const alt = media.alt_text?.trim() ?? '';
                        editorRef.current?.insertImage({ src: `media:${media.id}`, alt });
                    }
                }}
            />

            {/* 取消发布确认弹窗 */}
            <Modal
                open={unpublishModalOpen}
                onClose={() => setUnpublishModalOpen(false)}
                role="alertdialog"
                title="取消发布这篇文章？"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setUnpublishModalOpen(false)}>
                            保留在线
                        </Button>
                        <Button
                            variant="danger"
                            disabled={unpublishMutation.isPending || !post}
                            onClick={() => {
                                if (!post) return;
                                unpublishMutation.mutate(post.publication.publication_version);
                            }}
                        >
                            {unpublishMutation.isPending ? '提交中…' : '确认取消发布'}
                        </Button>
                    </>
                }
            >
                <div className="space-y-2 text-[14px] leading-6 text-ui-sub">
                    <p>下线后访客将无法访问这篇文章。草稿会保留，可随时重新发布。</p>
                </div>
            </Modal>
        </div>
    );
}
