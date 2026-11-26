import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { ArrowLeft, Columns2, PenLine, SlidersHorizontal } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TiptapMarkdownEditor, type TiptapMarkdownEditorHandle } from './tiptap-markdown-editor';
import { AuthenticatedImage, SafeHtmlPreview } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import { ErrorBanner, LoadingCard } from '@/components/ui/feedback';
import { Drawer, Modal } from '@/components/ui/overlay';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { publishJobStatusMeta, reviewStatusChip } from '@/features/dashboard/dashboard-page';
import { MediaPickerDrawer } from '@/features/media/media-picker-drawer';
import { apiClient, unwrapResponse, uploadMediaFile } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId, getMediaRejectionMessage, isApiError } from '@/lib/errors';
import { formatGeneration, formatRelativeTime, utf8ByteLength } from '@/lib/format';
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

export function PostEditorPage({ postId }: { postId?: string }) {
    const isCreateMode = !postId;
    const navigate = useNavigate();
    const queryClient = useQueryClient();
    const { toast } = useToast();
    const { user } = useAuth();

    const canSubmitReview = hasPermission(user, PERMISSIONS.POSTS_SUBMIT_OWN);
    const canPublish = hasPermission(user, PERMISSIONS.POSTS_PUBLISH);
    const canUploadMedia = hasAnyPermission(user, [
        PERMISSIONS.MEDIA_MANAGE_OWN,
        PERMISSIONS.MEDIA_MANAGE,
    ]);

    const editorRef = useRef<TiptapMarkdownEditorHandle>(null);
    const titleInputRef = useRef<HTMLInputElement>(null);
    const slugInputRef = useRef<HTMLInputElement>(null);
    const autoSaveTimerRef = useRef<number | null>(null);
    const hydratedPostIdRef = useRef<string | null>(null);
    const confirmedLeaveRef = useRef(false);

    const [draft, setDraft] = useState<DraftFields>(EMPTY_DRAFT);
    const [expectedRevisionId, setExpectedRevisionId] = useState<string | null>(null);
    const [currentRevNo, setCurrentRevNo] = useState<number>(0);
    const [currentReview, setCurrentReview] = useState<Schemas['ReviewSummary'] | null>(null);
    const [isDirty, setIsDirty] = useState(false);
    const [hasConflict, setHasConflict] = useState(false);
    const [saveState, setSaveState] = useState<SaveState>({
        kind: 'idle',
        text: isCreateMode ? '草稿尚未创建' : '正在读取文章…',
    });

    const [mobileTab, setMobileTab] = useState<'source' | 'preview'>('source');
    const [previewOpen, setPreviewOpen] = useState(false);
    const [settingsOpen, setSettingsOpen] = useState(false);
    const [settingsPromptedByCreate, setSettingsPromptedByCreate] = useState(false);
    const [slugError, setSlugError] = useState<string | null>(null);
    const [leaveModalOpen, setLeaveModalOpen] = useState(false);
    const [pickerMode, setPickerMode] = useState<'body' | 'cover' | null>(null);
    const [submitModalOpen, setSubmitModalOpen] = useState(false);
    const [publishModalOpen, setPublishModalOpen] = useState(false);
    const [unpublishModalOpen, setUnpublishModalOpen] = useState(false);
    const [selectedPublishRevId, setSelectedPublishRevId] = useState<string>('');
    const [pendingJob, setPendingJob] = useState<{
        jobId: string;
        generation: number;
        summary: string;
    } | null>(null);

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

    const revisionsQuery = useQuery({
        queryKey: queryKeys.posts.revisions(postId ?? '', 1, 100),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}/revisions', {
                    params: { path: { id: postId! }, query: { page: 1, per_page: 100 } },
                }),
            ),
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

    // 轮询待上线发布任务
    const pendingJobQuery = useQuery({
        queryKey: queryKeys.publishing.job(pendingJob?.jobId ?? ''),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/publishing/jobs/{id}', {
                    params: { path: { id: pendingJob!.jobId } },
                }),
            );
            return res.data;
        },
        enabled: Boolean(pendingJob?.jobId),
        refetchInterval: (query) => {
            const st = query.state.data?.status;
            if (!st || st === 'queued' || st === 'building') return 3000;
            return false;
        },
    });

    useEffect(() => {
        const st = pendingJobQuery.data?.status;
        if (st === 'active' || st === 'superseded' || st === 'retired') {
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            }
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
        }
    }, [pendingJobQuery.data?.status, postId, queryClient]);

    // 首次加载文章数据时同步到编辑表单
    useEffect(() => {
        const post = postQuery.data;
        if (!post) return;
        if (hydratedPostIdRef.current !== post.id) {
            hydratedPostIdRef.current = post.id;
            setDraft(postToDraft(post));
            setExpectedRevisionId(post.current_revision_id);
            setCurrentRevNo(post.current_revision.revision_no);
            setCurrentReview(post.current_revision.review);
            setIsDirty(false);
            setHasConflict(false);
            setSaveState({
                kind: 'saved',
                text: `已保存为 r${post.current_revision.revision_no} · ${formatRelativeTime(post.current_revision.created_at)}`,
            });
        }
    }, [postQuery.data]);

    // 清理定时器
    useEffect(() => {
        return () => {
            if (autoSaveTimerRef.current !== null) {
                window.clearTimeout(autoSaveTimerRef.current);
            }
        };
    }, []);

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
            confirmedLeaveRef.current = true;
            setSettingsOpen(false);
            setSettingsPromptedByCreate(false);
            setSlugError(null);
            toast('文章已创建并保存为 r1', 'ok');
            queryClient.setQueryData(queryKeys.posts.detail(created.id), created);
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            void navigate({ to: '/posts/$postId/edit', params: { postId: created.id } });
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
                setSettingsPromptedByCreate(true);
                setSettingsOpen(true);
                window.requestAnimationFrame(() => slugInputRef.current?.focus());
            }
            toast(msg, 'bad');
        },
    });

    // 保存新修订（含自动保存）
    const saveRevisionMutation = useMutation({
        mutationFn: async (payload: { postId: string; expectedId: string; draft: DraftFields }) => {
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
        onSuccess: (revision) => {
            setExpectedRevisionId(revision.id);
            setCurrentRevNo(revision.revision_no);
            setCurrentReview(revision.review);
            setIsDirty(false);
            setHasConflict(false);
            setSaveState({
                kind: 'saved',
                text: `已保存为 r${revision.revision_no} · 刚刚`,
            });
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
                void queryClient.invalidateQueries({
                    queryKey: queryKeys.posts.revisions(postId, 1, 100),
                });
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            }
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'revision_conflict') {
                // 收到 409 revision_conflict 时立即停止自动保存，保留本地输入
                if (autoSaveTimerRef.current !== null) {
                    window.clearTimeout(autoSaveTimerRef.current);
                    autoSaveTimerRef.current = null;
                }
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

    const triggerSaveRevision = useCallback(
        (snapshot: DraftFields) => {
            if (!postId || !expectedRevisionId || hasConflict) return;
            if (!snapshot.title.trim()) {
                setSaveState({ kind: 'failed', text: '请先填写文章标题后再保存。' });
                return;
            }
            if (utf8ByteLength(snapshot.markdown_body) > 524_288) {
                setSaveState({ kind: 'failed', text: 'Markdown 正文超出 512 KiB 限制。' });
                return;
            }
            setSaveState({ kind: 'saving', text: '保存中…' });
            saveRevisionMutation.mutate({
                postId,
                expectedId: expectedRevisionId,
                draft: snapshot,
            });
        },
        [postId, expectedRevisionId, hasConflict, saveRevisionMutation],
    );

    const scheduleAutoSave = useCallback(
        (nextDraft: DraftFields) => {
            setIsDirty(true);
            if (isCreateMode) {
                setSaveState({ kind: 'dirty', text: '草稿未创建' });
                return;
            }
            if (hasConflict) {
                setSaveState({ kind: 'conflict', text: '有新版本，已停止自动保存' });
                return;
            }
            setSaveState({ kind: 'dirty', text: '有未保存的更改' });
            if (autoSaveTimerRef.current !== null) {
                window.clearTimeout(autoSaveTimerRef.current);
            }
            autoSaveTimerRef.current = window.setTimeout(() => {
                triggerSaveRevision(nextDraft);
            }, 1500);
        },
        [isCreateMode, hasConflict, triggerSaveRevision],
    );

    const patchDraft = useCallback(
        (updater: (prev: DraftFields) => DraftFields) => {
            setDraft((prev) => {
                const next = updater(prev);
                scheduleAutoSave(next);
                return next;
            });
        },
        [scheduleAutoSave],
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
            toast('Slug 已更新', 'ok');
            queryClient.setQueryData(queryKeys.posts.detail(updated.id), updated);
            setDraft((prev) => ({ ...prev, slug: updated.slug }));
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    // 提交审核
    const submitReviewMutation = useMutation({
        mutationFn: async (revisionId: string) => {
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/posts/{id}/reviews', {
                    params: { path: { id: postId! } },
                    body: { revision_id: revisionId },
                }),
            );
            return res.data;
        },
        onSuccess: (review) => {
            setCurrentReview({ id: review.id, status: review.status });
            setSubmitModalOpen(false);
            toast(`已提交 r${review.revision_no} 审核`, 'ok');
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
                void queryClient.invalidateQueries({
                    queryKey: queryKeys.posts.revisions(postId, 1, 100),
                });
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            }
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    // 发布（仅已批准修订）
    const publishMutation = useMutation({
        mutationFn: async (params: { revisionId: string; expectedVersion: number }) => {
            const res = await unwrapResponse(
                apiClient.PUT('/api/v1/admin/posts/{id}/publication', {
                    params: { path: { id: postId! } },
                    body: {
                        revision_id: params.revisionId,
                        expected_publication_version: params.expectedVersion,
                    },
                }),
            );
            return res.data;
        },
        onSuccess: (accepted) => {
            setPublishModalOpen(false);
            const revNo = revisions.find((r) => r.id === selectedPublishRevId)?.revision_no;
            setPendingJob({
                jobId: accepted.publish_job_id,
                generation: accepted.desired_generation,
                summary: `发布 ${revNo ? `r${revNo}` : '所选修订'}：站点版本 ${formatGeneration(accepted.desired_generation)} 已排队。上线前访客仍看到当前线上版本。`,
            });
            toast('已接受发布，正在排队构建站点', 'ok');
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
                void queryClient.invalidateQueries({ queryKey: queryKeys.publishing.all });
            }
        },
        onError: (error) => {
            if (isApiError(error) && error.code === 'publication_conflict' && postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

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
        onSuccess: (accepted) => {
            setUnpublishModalOpen(false);
            setPendingJob({
                jobId: accepted.publish_job_id,
                generation: accepted.desired_generation,
                summary: `取消发布：站点版本 ${formatGeneration(accepted.desired_generation)} 已排队。新版本激活后文章将在公开站点下线。`,
            });
            toast('已提交取消发布，正在排队构建站点', 'ok');
            if (postId) {
                void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
                void queryClient.invalidateQueries({ queryKey: queryKeys.publishing.all });
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
        setSettingsPromptedByCreate(false);
        setSettingsOpen(true);
    }, []);

    const closeSettingsDrawer = useCallback(() => {
        if (pickerMode !== null) return;
        setSettingsOpen(false);
        setSettingsPromptedByCreate(false);
    }, [pickerMode]);

    const handleBackClick = useCallback(() => {
        if (hasContentToConfirmLeave) {
            setLeaveModalOpen(true);
            return;
        }
        void navigate({ to: '/posts' });
    }, [hasContentToConfirmLeave, navigate]);

    const confirmLeave = useCallback(() => {
        confirmedLeaveRef.current = true;
        setLeaveModalOpen(false);
        void navigate({ to: '/posts' });
    }, [navigate]);

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
            }
        },
        [canUploadMedia, queryClient, toast],
    );

    const handleCreatePost = useCallback(() => {
        if (!draft.title.trim()) {
            setSettingsOpen(false);
            setMobileTab('source');
            toast('请先输入文章标题', 'warn');
            window.requestAnimationFrame(() => titleInputRef.current?.focus());
            return;
        }
        if (!draft.slug.trim()) {
            setSlugError('请填写 Slug 后再创建文章');
            setSettingsPromptedByCreate(true);
            setSettingsOpen(true);
            window.requestAnimationFrame(() => slugInputRef.current?.focus());
            return;
        }
        if (!isValidSlug(draft.slug)) {
            setSlugError(
                'Slug 仅允许小写字母、数字和单个连字符，不能以连字符开头或结尾（1–100 字符）',
            );
            setSettingsPromptedByCreate(true);
            setSettingsOpen(true);
            window.requestAnimationFrame(() => slugInputRef.current?.focus());
            return;
        }
        if (utf8ByteLength(draft.markdown_body) > 524_288) {
            toast('Markdown 正文超出 512 KiB 限制', 'warn');
            return;
        }
        setSlugError(null);
        createPostMutation.mutate(draft);
    }, [draft, createPostMutation, toast]);

    const post = postQuery.data;
    const revisions = revisionsQuery.data?.data ?? [];
    const approvedRevisions = revisions.filter((rev) => rev.review?.status === 'approved');
    const revNoById = new Map<string, number>();
    for (const rev of revisions) {
        revNoById.set(rev.id, rev.revision_no);
    }
    if (post) {
        revNoById.set(post.current_revision.id, post.current_revision.revision_no);
    }

    const previewHtml = useMemo(
        () => renderSafeMarkdown(draft.markdown_body),
        [draft.markdown_body],
    );

    if (!isCreateMode && postQuery.isLoading) {
        return <LoadingCard text="正在加载文章内容…" />;
    }

    if (!isCreateMode && postQuery.isError) {
        return (
            <ErrorBanner
                title="无法读取文章"
                error={postQuery.error}
                onRetry={() => void postQuery.refetch()}
            />
        );
    }

    const desiredRevId = post?.publication.desired_published_revision_id ?? null;
    const liveRevId = post?.publication.live_revision_id ?? null;
    const desiredRevNo = desiredRevId ? revNoById.get(desiredRevId) : undefined;
    const liveRevNo = liveRevId ? revNoById.get(liveRevId) : undefined;

    const editChip = currentRevNo
        ? reviewStatusChip(currentRevNo, currentReview)
        : { label: '未保存', tone: 'mute' as const };

    const canSubmitNow =
        !isCreateMode &&
        Boolean(expectedRevisionId) &&
        !isDirty &&
        !hasConflict &&
        currentReview === null &&
        saveState.kind !== 'saving';

    const canUnpublishNow =
        !isCreateMode && Boolean(post?.publication.desired_published_revision_id);

    const reloadLatestAfterConflict = async () => {
        if (!postId) return;
        const result = await postQuery.refetch();
        if (result.data) {
            setDraft(postToDraft(result.data));
            setExpectedRevisionId(result.data.current_revision_id);
            setCurrentRevNo(result.data.current_revision.revision_no);
            setCurrentReview(result.data.current_revision.review);
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
    const pendingJobData = pendingJobQuery.data;
    const pendingStatusMeta = pendingJobData ? publishJobStatusMeta(pendingJobData.status) : null;
    const needsRequiredSettings = isCreateMode && !isValidSlug(draft.slug);

    return (
        <div className="space-y-3">
            {/* 顶栏操作区：与文章标题分离，仅承载返回导航、状态与全局操作 */}
            <header
                aria-label="文章操作栏"
                className="flex flex-wrap items-center justify-between gap-2.5 rounded-xl border border-ui-line bg-white px-3.5 py-2.5 shadow-[var(--shadow-card)] sm:px-4"
            >
                <div className="flex min-w-0 items-center gap-2.5">
                    <button
                        type="button"
                        onClick={handleBackClick}
                        aria-label="返回文章列表"
                        className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg border border-ui-line bg-ui-subtle px-2.5 text-[13px] font-medium text-ui-sub transition-colors hover:border-ui-line-strong hover:bg-white hover:text-ui-text"
                    >
                        <ArrowLeft className="h-3.5 w-3.5" />
                        <span>返回</span>
                    </button>

                    <span className="h-4 w-px shrink-0 bg-ui-line" aria-hidden="true" />

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
                        {needsRequiredSettings ? (
                            <span
                                aria-hidden="true"
                                title="尚未填写 Slug"
                                className="h-1.5 w-1.5 rounded-full bg-warn"
                            />
                        ) : null}
                    </Button>

                    {isCreateMode ? (
                        <Button
                            variant="primary"
                            size="sm"
                            disabled={createPostMutation.isPending}
                            onClick={handleCreatePost}
                        >
                            {createPostMutation.isPending ? '创建中…' : '创建文章'}
                        </Button>
                    ) : (
                        <>
                            <Button
                                variant="secondary"
                                size="sm"
                                disabled={!isDirty || hasConflict || saveRevisionMutation.isPending}
                                onClick={() => triggerSaveRevision(draft)}
                            >
                                保存修订
                            </Button>
                            <Link
                                to="/posts/$postId/revisions"
                                params={{ postId: postId! }}
                                className="inline-flex h-8 items-center rounded-[8px] border border-ui-line bg-white px-3 text-[13px] font-medium text-ui-text shadow-2xs transition-colors hover:border-ui-line-strong hover:bg-ui-subtle"
                            >
                                修订历史
                            </Link>
                            {canSubmitReview ? (
                                <Button
                                    variant="secondary"
                                    size="sm"
                                    className="border-accent/40 text-accent hover:bg-accent-tint"
                                    disabled={!canSubmitNow}
                                    title={
                                        isDirty
                                            ? '请等待保存完成后再提交审核'
                                            : hasConflict
                                              ? '请先解决修订冲突'
                                              : currentReview
                                                ? `r${currentRevNo} 已提交审核`
                                                : undefined
                                    }
                                    onClick={() => setSubmitModalOpen(true)}
                                >
                                    提交审核
                                </Button>
                            ) : null}
                            {canPublish ? (
                                <>
                                    <Button
                                        variant="danger-outline"
                                        size="sm"
                                        disabled={!canUnpublishNow}
                                        onClick={() => setUnpublishModalOpen(true)}
                                    >
                                        取消发布
                                    </Button>
                                    <Button
                                        variant="primary"
                                        size="sm"
                                        onClick={() => {
                                            setSelectedPublishRevId(approvedRevisions[0]?.id ?? '');
                                            setPublishModalOpen(true);
                                        }}
                                    >
                                        发布…
                                    </Button>
                                </>
                            ) : null}
                        </>
                    )}
                </div>
            </header>

            {/* 编辑既有文章时展示三种修订状态；新建文章页面不显示 */}
            {!isCreateMode ? (
                <section aria-label="修订状态" className="studio-card overflow-hidden">
                    <dl className="grid grid-cols-1 divide-y divide-ui-line text-[13px] sm:grid-cols-3 sm:divide-x sm:divide-y-0">
                        <div className="flex items-center justify-between gap-2 px-4 py-2.5 sm:block">
                            <dt className="text-ui-sub">当前编辑修订</dt>
                            <dd className="flex flex-wrap items-center gap-1.5 sm:mt-1">
                                <StatusChip tone={editChip.tone}>{editChip.label}</StatusChip>
                                {isDirty ? (
                                    <span className="text-[12px] text-warn">＋未保存更改</span>
                                ) : null}
                            </dd>
                        </div>
                        <div className="flex items-center justify-between gap-2 px-4 py-2.5 sm:block">
                            <dt className="text-ui-sub">期望公开修订</dt>
                            <dd className="sm:mt-1">
                                {desiredRevId ? (
                                    desiredRevId === liveRevId ? (
                                        <StatusChip tone="ok">
                                            {desiredRevNo ? `r${desiredRevNo} ` : ''}同线上
                                        </StatusChip>
                                    ) : (
                                        <StatusChip tone="info">
                                            {desiredRevNo ? `r${desiredRevNo} ` : ''}待上线
                                        </StatusChip>
                                    )
                                ) : liveRevId ? (
                                    <StatusChip tone="warn">无（取消发布 · 待上线）</StatusChip>
                                ) : (
                                    <span className="text-ui-sub">无</span>
                                )}
                            </dd>
                        </div>
                        <div className="flex items-center justify-between gap-2 px-4 py-2.5 sm:block">
                            <dt className="text-ui-sub">当前线上修订</dt>
                            <dd className="sm:mt-1">
                                {liveRevId ? (
                                    <StatusChip tone="ok">
                                        {liveRevNo ? `r${liveRevNo} ` : ''}已上线
                                    </StatusChip>
                                ) : (
                                    <span className="text-ui-sub">无（未上线）</span>
                                )}
                            </dd>
                        </div>
                    </dl>
                </section>
            ) : null}

            {/* 待上线任务横幅 */}
            {pendingJob ? (
                <output className="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-lg border border-[#CBD5E6] bg-accent-tint px-4 py-3 text-[14px] text-accent">
                    <StatusChip tone={pendingStatusMeta?.tone ?? 'info'} className="bg-white">
                        {pendingStatusMeta?.label ?? '待上线'}
                    </StatusChip>
                    <span>{pendingJob.summary}</span>
                    <Link
                        to="/publishing/$jobId"
                        params={{ jobId: pendingJob.jobId }}
                        className="font-medium underline underline-offset-2"
                    >
                        查看任务详情
                    </Link>
                </output>
            ) : null}

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
            <div
                className={cn(
                    'grid gap-3',
                    previewOpen ? 'lg:grid-cols-2' : 'lg:mx-auto lg:w-full lg:max-w-[980px]',
                )}
            >
                {/* 文章标题 + Tiptap Markdown 编辑器 */}
                <section
                    aria-label="正文编辑器"
                    className={cn(
                        'studio-card flex min-h-[500px] flex-col overflow-hidden',
                        isCreateMode
                            ? 'lg:h-[calc(100vh-10rem)] lg:min-h-[560px]'
                            : 'lg:h-[calc(100vh-14.5rem)] lg:min-h-[520px]',
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
                        'studio-card flex min-h-[500px] flex-col overflow-hidden',
                        isCreateMode
                            ? 'lg:h-[calc(100vh-10rem)] lg:min-h-[560px]'
                            : 'lg:h-[calc(100vh-14.5rem)] lg:min-h-[520px]',
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
                subtitle="随修订冻结发布"
                widthClassName="sm:w-[440px]"
                footer={
                    isCreateMode ? (
                        <div className="flex items-center justify-end gap-2">
                            <Button variant="secondary" onClick={closeSettingsDrawer}>
                                稍后设置
                            </Button>
                            <Button
                                variant="primary"
                                disabled={createPostMutation.isPending}
                                onClick={handleCreatePost}
                            >
                                {createPostMutation.isPending ? '创建中…' : '创建文章'}
                            </Button>
                        </div>
                    ) : (
                        <div className="flex items-center justify-between gap-2">
                            <span className="text-[12px] text-ui-sub">修改后将随修订自动保存</span>
                            <Button variant="secondary" onClick={closeSettingsDrawer}>
                                完成
                            </Button>
                        </div>
                    )
                }
            >
                <div id="post-settings-drawer" className="space-y-5 text-[14px]">
                    {settingsPromptedByCreate && isCreateMode ? (
                        <output className="block rounded-lg border border-[#EBD7A9] bg-warn-tint px-3.5 py-2.5 text-[13px] leading-5 text-warn">
                            创建文章前请先完善必填项 <strong className="font-semibold">Slug</strong>
                            ，也可以在此补充摘要、分类、标签与封面。
                        </output>
                    ) : null}

                    {/* Slug */}
                    <div>
                        <label htmlFor="post-slug" className="block font-medium text-ui-text">
                            Slug
                            {isCreateMode ? (
                                <span className="ml-1 text-bad" aria-hidden="true">
                                    *
                                </span>
                            ) : null}
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
                                : '仅允许小写字母、数字和连字符，首次被接受发布后锁定。'}
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

            {/* 提交审核弹窗 */}
            <Modal
                open={submitModalOpen}
                onClose={() => setSubmitModalOpen(false)}
                title={`提交 r${currentRevNo} 审核？`}
                description="审核只作用于这一修订。之后的编辑会产生新修订，不继承这次的审核结果。提交后当前线上内容不受影响。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setSubmitModalOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={submitReviewMutation.isPending || !expectedRevisionId}
                            onClick={() => {
                                if (expectedRevisionId) {
                                    submitReviewMutation.mutate(expectedRevisionId);
                                }
                            }}
                        >
                            {submitReviewMutation.isPending
                                ? '提交中…'
                                : `提交 r${currentRevNo} 审核`}
                        </Button>
                    </>
                }
            >
                <p className="rounded-md bg-ui-bg px-3 py-2 text-[13px] text-ui-sub">
                    修订标题：<span className="text-ui-text">{draft.title}</span>
                </p>
            </Modal>

            {/* 发布选择弹窗（仅列出已批准修订） */}
            <Modal
                open={publishModalOpen}
                onClose={() => setPublishModalOpen(false)}
                title="选择要发布的修订"
                description={`只列出已批准的修订。${
                    currentReview?.status !== 'approved'
                        ? `当前编辑的 r${currentRevNo} 尚未批准，不能发布。`
                        : ''
                }发布后进入站点构建，新版本激活后才算上线。`}
                maxWidthClassName="max-w-[500px]"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setPublishModalOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={
                                !selectedPublishRevId ||
                                approvedRevisions.length === 0 ||
                                publishMutation.isPending ||
                                !post
                            }
                            onClick={() => {
                                if (!post || !selectedPublishRevId) return;
                                publishMutation.mutate({
                                    revisionId: selectedPublishRevId,
                                    expectedVersion: post.publication.publication_version,
                                });
                            }}
                        >
                            {publishMutation.isPending ? '提交中…' : '发布所选修订'}
                        </Button>
                    </>
                }
            >
                {approvedRevisions.length === 0 ? (
                    <p className="rounded-md bg-ui-bg px-3 py-4 text-[14px] text-ui-sub">
                        还没有已批准的修订。请先提交审核并批准后再发布。
                    </p>
                ) : (
                    <fieldset className="max-h-72 space-y-2 overflow-y-auto">
                        <legend className="sr-only">已批准的修订</legend>
                        {approvedRevisions.map((rev) => (
                            <label
                                key={rev.id}
                                className={cn(
                                    'flex cursor-pointer items-center gap-3 rounded-lg border px-3 py-2.5',
                                    selectedPublishRevId === rev.id
                                        ? 'border-accent bg-accent-tint/50'
                                        : 'border-ui-line',
                                )}
                            >
                                <input
                                    type="radio"
                                    name="publish-revision"
                                    checked={selectedPublishRevId === rev.id}
                                    onChange={() => setSelectedPublishRevId(rev.id)}
                                    className="accent-[#137A4D]"
                                />
                                <span className="min-w-0 flex-1 text-[14px]">
                                    <span className="tnum font-medium">r{rev.revision_no}</span>{' '}
                                    <span className="truncate text-ui-sub">
                                        · {formatRelativeTime(rev.created_at)}
                                    </span>
                                    <span className="block truncate text-[12px] text-ui-sub">
                                        {rev.title}
                                    </span>
                                </span>
                                <span className="flex shrink-0 gap-1">
                                    {desiredRevId === rev.id ? (
                                        <StatusChip tone="info">期望公开</StatusChip>
                                    ) : null}
                                    {liveRevId === rev.id ? (
                                        <StatusChip tone="ok">线上</StatusChip>
                                    ) : null}
                                </span>
                            </label>
                        ))}
                    </fieldset>
                )}
            </Modal>

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
                    <p>
                        线上的{' '}
                        <span className="tnum text-ui-text">
                            {liveRevNo ? `r${liveRevNo}` : '当前版本'}
                        </span>{' '}
                        会在包含此变更的新站点版本激活后下线：详情页返回 404，列表、sitemap 与 RSS
                        移除该文章。
                    </p>
                    <p>
                        在那之前访客仍能看到它。草稿与修订历史都会保留，之后可以重新发布已批准的修订。
                    </p>
                </div>
            </Modal>
        </div>
    );
}
