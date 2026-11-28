import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Copy, ExternalLink, Trash2, Upload } from 'lucide-react';
import { useRef, useState } from 'react';
import { AuthenticatedImage } from '@/components/media/authenticated-media';
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
import { Drawer, Modal } from '@/components/ui/overlay';
import { StatusChip, type ChipTone } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, unwrapResponse, uploadMediaFile } from '@/lib/api';
import { formatErrorWithRequestId, getMediaRejectionMessage, isApiError } from '@/lib/errors';
import { formatBytes, formatDateTime, shortUuid } from '@/lib/format';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

const ALLOWED_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp']);
const MAX_FILE_BYTES = 10 * 1024 * 1024;

const MEDIA_STATUS_META: Record<Schemas['MediaStatus'], { label: string; tone: ChipTone }> = {
    ready: { label: '就绪', tone: 'ok' },
    pending: { label: '校验中', tone: 'warn' },
    rejected: { label: '未通过', tone: 'bad' },
};

interface UploadQueueItem {
    key: string;
    filename: string;
    state: 'uploading' | 'ready' | 'failed';
    message: string;
}

export function MediaLibraryPage() {
    const { user } = useAuth();
    const { toast } = useToast();
    const queryClient = useQueryClient();
    const fileInputRef = useRef<HTMLInputElement>(null);

    const canManageAll = hasPermission(user, PERMISSIONS.MEDIA_MANAGE);

    const [page, setPage] = useState(1);
    const [statusFilter, setStatusFilter] = useState<Schemas['MediaStatus'] | ''>('');
    const [queue, setQueue] = useState<UploadQueueItem[]>([]);
    const [activeMedia, setActiveMedia] = useState<Schemas['Media'] | null>(null);
    const [altTextInput, setAltTextInput] = useState('');
    const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
    const [deleteConflictUsage, setDeleteConflictUsage] = useState<Schemas['MediaUsage'] | null>(
        null,
    );

    const openMediaDrawer = (media: Schemas['Media']) => {
        setActiveMedia(media);
        setAltTextInput(media.alt_text ?? '');
        setDeleteConflictUsage(null);
    };

    const mediaDetailQuery = useQuery({
        queryKey: queryKeys.media.detail(activeMedia?.id ?? ''),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/media/{id}', {
                    params: { path: { id: activeMedia!.id } },
                }),
            ),
        enabled: activeMedia !== null,
    });

    const queryParams = {
        page,
        per_page: 20,
        status: statusFilter || undefined,
    };

    const mediaQuery = useQuery({
        queryKey: queryKeys.media.list(queryParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/media', {
                    params: { query: queryParams },
                }),
            ),
    });

    const updateAltMutation = useMutation({
        mutationFn: async (input: { id: string; alt_text: string }) => {
            const res = await unwrapResponse(
                apiClient.PATCH('/api/v1/admin/media/{id}', {
                    params: { path: { id: input.id } },
                    body: { alt_text: input.alt_text },
                }),
            );
            return res.data;
        },
        onSuccess: (updated) => {
            setActiveMedia(updated);
            toast('替代文本已保存', 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.media.all });
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const deleteMutation = useMutation({
        mutationFn: async (id: string) => {
            await unwrapResponse(
                apiClient.DELETE('/api/v1/admin/media/{id}', {
                    params: { path: { id } },
                }),
            );
        },
        onSuccess: () => {
            setDeleteConfirmOpen(false);
            setDeleteConflictUsage(null);
            setActiveMedia(null);
            toast('媒体已删除', 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.media.all });
        },
        onError: (error) => {
            setDeleteConfirmOpen(false);
            if (isApiError(error) && error.code === 'media_in_use' && error.mediaUsage) {
                setDeleteConflictUsage(error.mediaUsage);
            }
            if (activeMedia) {
                void queryClient.invalidateQueries({
                    queryKey: queryKeys.media.detail(activeMedia.id),
                });
            }
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const handleFiles = async (files: FileList | File[]) => {
        const list = Array.from(files);
        for (const file of list) {
            const key = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

            if (file.size > MAX_FILE_BYTES) {
                setQueue((prev) => [
                    {
                        key,
                        filename: file.name,
                        state: 'failed',
                        message: '文件超过 10 MiB 限制，请压缩后重新上传。',
                    },
                    ...prev,
                ]);
                continue;
            }
            if (file.type && !ALLOWED_TYPES.has(file.type)) {
                setQueue((prev) => [
                    {
                        key,
                        filename: file.name,
                        state: 'failed',
                        message: '仅支持 JPEG、PNG 和 WebP 格式图片。',
                    },
                    ...prev,
                ]);
                continue;
            }

            setQueue((prev) => [
                {
                    key,
                    filename: file.name,
                    state: 'uploading',
                    message: '正在上传并等待服务端校验图片内容与尺寸…',
                },
                ...prev,
            ]);

            try {
                const uploaded = await uploadMediaFile(file);
                setQueue((prev) =>
                    prev.map((item) =>
                        item.key === key
                            ? {
                                  ...item,
                                  state: 'ready',
                                  message: `已就绪（${uploaded.media.width ?? '?'} × ${uploaded.media.height ?? '?'}），可插入文章或设为封面。`,
                              }
                            : item,
                    ),
                );
                void queryClient.invalidateQueries({ queryKey: queryKeys.media.all });
            } catch (error) {
                setQueue((prev) =>
                    prev.map((item) =>
                        item.key === key
                            ? {
                                  ...item,
                                  state: 'failed',
                                  message: formatErrorWithRequestId(error),
                              }
                            : item,
                    ),
                );
            }
        }
    };

    const items = mediaQuery.data?.data ?? [];
    const pagination = mediaQuery.data?.meta.pagination;

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="ASSETS"
                title="媒体库"
                description="只有校验通过、状态为「就绪」的媒体才能插入文章或设为封面。支持 JPEG、PNG、WebP，单文件不超过 10 MiB。"
                actions={
                    <>
                        <input
                            ref={fileInputRef}
                            type="file"
                            multiple
                            accept="image/jpeg,image/png,image/webp"
                            className="sr-only"
                            onChange={(event) => {
                                if (event.target.files?.length) {
                                    void handleFiles(event.target.files);
                                }
                                event.target.value = '';
                            }}
                        />
                        <Button variant="primary" onClick={() => fileInputRef.current?.click()}>
                            <Upload className="h-4 w-4" />
                            上传图片
                        </Button>
                    </>
                }
            />

            {!canManageAll ? (
                <p className="rounded-xl border border-[#BBDCD8] bg-accent-tint px-4 py-2.5 text-[13px] text-accent-2">
                    你只能查看和管理自己上传的媒体。
                </p>
            ) : null}

            {/* 上传队列 */}
            {queue.length > 0 ? (
                <section aria-label="上传队列" className="studio-card overflow-hidden">
                    <header className="flex items-center justify-between border-b border-ui-line bg-ui-subtle/70 px-4 py-2.5 text-[13px] text-ui-sub sm:px-5">
                        <span className="font-medium text-ui-text">本次上传队列</span>
                        <button
                            type="button"
                            onClick={() => setQueue([])}
                            className="font-medium text-accent hover:underline"
                        >
                            清空记录
                        </button>
                    </header>
                    <ul className="divide-y divide-ui-line text-[13.5px]" aria-live="polite">
                        {queue.map((entry) => (
                            <li
                                key={entry.key}
                                className="flex flex-wrap items-center justify-between gap-2 px-4 py-3 sm:px-5"
                            >
                                <div className="min-w-0 flex-1">
                                    <div className="truncate font-medium text-ui-text">
                                        {entry.filename}
                                    </div>
                                    <p
                                        className={
                                            entry.state === 'failed'
                                                ? 'mt-0.5 text-[12px] text-bad'
                                                : 'mt-0.5 text-[12px] text-ui-sub'
                                        }
                                    >
                                        {entry.message}
                                    </p>
                                </div>
                                <StatusChip
                                    tone={
                                        entry.state === 'ready'
                                            ? 'ok'
                                            : entry.state === 'failed'
                                              ? 'bad'
                                              : 'info'
                                    }
                                >
                                    {entry.state === 'ready'
                                        ? '就绪'
                                        : entry.state === 'failed'
                                          ? '失败'
                                          : '上传校验中'}
                                </StatusChip>
                            </li>
                        ))}
                    </ul>
                </section>
            ) : null}

            {/* 媒体画廊卡片 */}
            <section className="studio-card overflow-hidden">
                <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ui-line bg-ui-subtle/60 px-4 py-3 sm:px-5">
                    <div className="flex items-center gap-2.5">
                        <Select
                            id="media-status-filter"
                            ariaLabel="按状态筛选"
                            value={statusFilter}
                            onChange={(nextValue) => {
                                setStatusFilter(nextValue as Schemas['MediaStatus'] | '');
                                setPage(1);
                            }}
                            options={[
                                { value: '', label: '全部状态' },
                                { value: 'ready', label: '就绪' },
                                { value: 'pending', label: '校验中' },
                                { value: 'rejected', label: '未通过' },
                            ]}
                        />
                    </div>
                    {pagination ? (
                        <span className="tnum text-[12.5px] text-ui-sub">
                            共 {pagination.total} 个资源
                        </span>
                    ) : null}
                </div>

                {mediaQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            title="媒体列表加载失败"
                            error={mediaQuery.error}
                            onRetry={() => void mediaQuery.refetch()}
                        />
                    </div>
                ) : mediaQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载媒体库…" />
                    </div>
                ) : items.length === 0 ? (
                    <EmptyState
                        className="border-0 shadow-none"
                        title="还没有媒体文件"
                        description="上传的图片经服务端校验通过后，即可插入文章或设为封面。"
                        action={
                            <Button variant="primary" onClick={() => fileInputRef.current?.click()}>
                                <Upload className="h-4 w-4" />
                                上传图片
                            </Button>
                        }
                    />
                ) : (
                    <>
                        <ul className="grid grid-cols-2 gap-4 p-4 sm:grid-cols-3 sm:p-5 lg:grid-cols-4 xl:grid-cols-5">
                            {items.map((item) => {
                                const statusMeta = MEDIA_STATUS_META[item.status];
                                return (
                                    <li key={item.id}>
                                        <button
                                            type="button"
                                            onClick={() => openMediaDrawer(item)}
                                            className="group w-full overflow-hidden rounded-xl border border-ui-line bg-white text-left transition-all hover:border-accent/60 hover:shadow-[0_4px_12px_rgba(9,28,26,0.06)]"
                                        >
                                            <div className="aspect-[4/3] overflow-hidden bg-[#EEF3F2]">
                                                {item.status === 'ready' ? (
                                                    <AuthenticatedImage
                                                        mediaId={item.id}
                                                        alt={
                                                            item.alt_text || item.original_filename
                                                        }
                                                        className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
                                                    />
                                                ) : (
                                                    <div className="grid h-full w-full place-items-center text-[12px] text-ui-sub">
                                                        {statusMeta.label}
                                                    </div>
                                                )}
                                            </div>
                                            <div className="p-2.5">
                                                <div className="truncate text-[13px] font-medium text-ui-text group-hover:text-accent">
                                                    {item.original_filename}
                                                </div>
                                                <div className="mt-1.5 flex items-center justify-between gap-2">
                                                    <StatusChip tone={statusMeta.tone}>
                                                        {statusMeta.label}
                                                    </StatusChip>
                                                    <span className="tnum text-[11.5px] text-ui-sub">
                                                        {formatBytes(
                                                            item.size_bytes ??
                                                                item.declared_size_bytes,
                                                        )}
                                                    </span>
                                                </div>
                                            </div>
                                        </button>
                                    </li>
                                );
                            })}
                        </ul>

                        {pagination ? (
                            <PaginationBar
                                page={pagination.page}
                                perPage={pagination.per_page}
                                total={pagination.total}
                                totalPages={pagination.total_pages}
                                unit="个文件"
                                onPageChange={setPage}
                            />
                        ) : null}
                    </>
                )}
            </section>

            {/* 媒体详情抽屉 */}
            <Drawer
                open={activeMedia !== null}
                onClose={() => setActiveMedia(null)}
                title={activeMedia?.original_filename ?? '媒体详情'}
                subtitle={activeMedia?.id}
                footer={
                    activeMedia ? (
                        <div className="space-y-2">
                            <div className="flex flex-wrap items-center justify-between gap-2">
                                <Button
                                    variant="ghost"
                                    className="text-bad hover:bg-bad-tint hover:text-bad"
                                    onClick={() => setDeleteConfirmOpen(true)}
                                >
                                    <Trash2 className="h-4 w-4" />
                                    删除
                                </Button>
                                <div className="flex gap-2">
                                    {activeMedia.status === 'ready' ? (
                                        <Button
                                            variant="secondary"
                                            onClick={async () => {
                                                const alt =
                                                    altTextInput.trim() ||
                                                    activeMedia.original_filename;
                                                await navigator.clipboard?.writeText(
                                                    `![${alt}](media:${activeMedia.id})`,
                                                );
                                                toast('已复制 Markdown 媒体引用', 'ok');
                                            }}
                                        >
                                            <Copy className="h-3.5 w-3.5" />
                                            复制引用
                                        </Button>
                                    ) : null}
                                    <Button
                                        variant="primary"
                                        disabled={updateAltMutation.isPending}
                                        onClick={() =>
                                            updateAltMutation.mutate({
                                                id: activeMedia.id,
                                                alt_text: altTextInput,
                                            })
                                        }
                                    >
                                        {updateAltMutation.isPending ? '保存中…' : '保存'}
                                    </Button>
                                </div>
                            </div>
                            <p className="text-[12px] text-ui-sub">
                                被文章修订、系列封面、个人资料或当前线上站点引用时服务端会拒绝删除。
                            </p>
                        </div>
                    ) : null
                }
            >
                {activeMedia ? (
                    <div className="space-y-5">
                        <div className="aspect-[4/3] overflow-hidden rounded-md border border-ui-line bg-ui-bg">
                            {activeMedia.status === 'ready' ? (
                                <AuthenticatedImage
                                    mediaId={activeMedia.id}
                                    alt={activeMedia.alt_text || activeMedia.original_filename}
                                    className="h-full w-full object-contain"
                                />
                            ) : (
                                <div className="grid h-full w-full place-items-center text-[13px] text-ui-sub">
                                    {MEDIA_STATUS_META[activeMedia.status].label}
                                </div>
                            )}
                        </div>

                        <dl className="grid grid-cols-[88px_1fr] gap-y-2.5 text-[14px]">
                            <dt className="text-ui-sub">文件名</dt>
                            <dd className="break-all">{activeMedia.original_filename}</dd>
                            <dt className="text-ui-sub">状态</dt>
                            <dd>
                                <StatusChip tone={MEDIA_STATUS_META[activeMedia.status].tone}>
                                    {MEDIA_STATUS_META[activeMedia.status].label}
                                </StatusChip>
                                {activeMedia.rejection_reason ? (
                                    <p className="mt-1 text-[12px] text-bad">
                                        {getMediaRejectionMessage(activeMedia.rejection_reason)}
                                    </p>
                                ) : null}
                            </dd>
                            <dt className="text-ui-sub">MIME</dt>
                            <dd className="font-code text-[13px]">
                                {activeMedia.mime_type ?? activeMedia.declared_mime_type}
                            </dd>
                            <dt className="text-ui-sub">大小</dt>
                            <dd className="tnum">
                                {formatBytes(
                                    activeMedia.size_bytes ?? activeMedia.declared_size_bytes,
                                )}
                            </dd>
                            <dt className="text-ui-sub">尺寸</dt>
                            <dd className="tnum">
                                {activeMedia.width && activeMedia.height
                                    ? `${activeMedia.width} × ${activeMedia.height}`
                                    : '—'}
                            </dd>
                            <dt className="text-ui-sub">上传时间</dt>
                            <dd className="tnum">{formatDateTime(activeMedia.created_at)}</dd>
                        </dl>

                        {/* 业务引用位置（含文章、头像、系列封面及当前线上系列封面） */}
                        {(() => {
                            const usage =
                                deleteConflictUsage ?? mediaDetailQuery.data?.meta ?? null;
                            if (mediaDetailQuery.isLoading && !usage) {
                                return (
                                    <div className="rounded-lg border border-ui-line bg-ui-subtle/50 px-3.5 py-2.5 text-[12.5px] text-ui-sub">
                                        正在加载引用位置…
                                    </div>
                                );
                            }
                            if (!usage) return null;
                            const hiddenCount = Math.max(
                                0,
                                usage.reference_count - usage.references.length,
                            );
                            return (
                                <section
                                    aria-label="媒体引用位置"
                                    className="rounded-lg border border-ui-line bg-ui-subtle/50 p-3.5 text-[13px]"
                                >
                                    <div className="flex items-center justify-between gap-2">
                                        <span className="font-medium text-ui-text">业务引用</span>
                                        <StatusChip
                                            tone={usage.reference_count > 0 ? 'warn' : 'ok'}
                                        >
                                            {usage.reference_count > 0
                                                ? `${usage.reference_count} 处引用`
                                                : '未被引用'}
                                        </StatusChip>
                                    </div>
                                    {usage.references.length > 0 ? (
                                        <ul className="mt-2.5 space-y-1.5 text-[12.5px]">
                                            {usage.references.map((loc, idx) => {
                                                const key = `${loc.kind}-${loc.post_id ?? ''}-${loc.revision_id ?? ''}-${loc.series_id ?? ''}-${idx}`;
                                                if (
                                                    (loc.kind === 'post_cover' ||
                                                        loc.kind === 'post_body') &&
                                                    loc.post_id
                                                ) {
                                                    return (
                                                        <li
                                                            key={key}
                                                            className="flex items-center justify-between gap-2 rounded-md bg-white px-2.5 py-1.5"
                                                        >
                                                            <span className="text-ui-text">
                                                                {loc.kind === 'post_cover'
                                                                    ? '文章封面'
                                                                    : '文章正文'}
                                                                {loc.revision_no
                                                                    ? ` · r${loc.revision_no}`
                                                                    : ''}
                                                            </span>
                                                            <Link
                                                                to="/posts/$postId/edit"
                                                                params={{ postId: loc.post_id }}
                                                                className="inline-flex items-center gap-1 font-code text-[11.5px] text-accent hover:underline"
                                                            >
                                                                {shortUuid(loc.post_id)}
                                                                <ExternalLink className="h-3 w-3" />
                                                            </Link>
                                                        </li>
                                                    );
                                                }
                                                if (loc.kind === 'series_cover' && loc.series_id) {
                                                    return (
                                                        <li
                                                            key={key}
                                                            className="flex items-center justify-between gap-2 rounded-md bg-white px-2.5 py-1.5"
                                                        >
                                                            <span className="text-ui-text">
                                                                系列封面
                                                            </span>
                                                            <Link
                                                                to="/series/$seriesId"
                                                                params={{
                                                                    seriesId: loc.series_id,
                                                                }}
                                                                className="inline-flex items-center gap-1 font-code text-[11.5px] text-accent hover:underline"
                                                            >
                                                                {shortUuid(loc.series_id)}
                                                                <ExternalLink className="h-3 w-3" />
                                                            </Link>
                                                        </li>
                                                    );
                                                }
                                                if (loc.kind === 'profile_avatar') {
                                                    return (
                                                        <li
                                                            key={key}
                                                            className="flex items-center justify-between gap-2 rounded-md bg-white px-2.5 py-1.5"
                                                        >
                                                            <span className="text-ui-text">
                                                                个人资料头像
                                                            </span>
                                                            <Link
                                                                to="/profile"
                                                                className="inline-flex items-center gap-1 text-[11.5px] text-accent hover:underline"
                                                            >
                                                                前往个人资料
                                                                <ExternalLink className="h-3 w-3" />
                                                            </Link>
                                                        </li>
                                                    );
                                                }
                                                return (
                                                    <li
                                                        key={key}
                                                        className="rounded-md bg-white px-2.5 py-1.5 text-ui-text"
                                                    >
                                                        {loc.kind === 'post_cover'
                                                            ? '文章封面'
                                                            : loc.kind === 'post_body'
                                                              ? '文章正文'
                                                              : '系列封面'}
                                                    </li>
                                                );
                                            })}
                                        </ul>
                                    ) : null}
                                    {hiddenCount > 0 ? (
                                        <p className="mt-2 text-[12px] text-ui-sub">
                                            另有 {hiddenCount} 处引用不在当前角色可见范围内。
                                        </p>
                                    ) : null}
                                </section>
                            );
                        })()}

                        <div>
                            <label
                                htmlFor="media-alt-input"
                                className="block text-[13px] font-medium text-ui-text"
                            >
                                替代文本（Alt）
                            </label>
                            <input
                                id="media-alt-input"
                                type="text"
                                value={altTextInput}
                                onChange={(event) => setAltTextInput(event.target.value)}
                                placeholder="为屏幕阅读器和无图环境描述图片内容"
                                className="studio-input mt-1.5"
                            />
                        </div>
                    </div>
                ) : null}
            </Drawer>

            {/* 删除确认弹窗 */}
            <Modal
                open={deleteConfirmOpen}
                onClose={() => setDeleteConfirmOpen(false)}
                role="alertdialog"
                title="删除这个媒体文件？"
                description="删除后无法恢复。如果该媒体正被文章草稿、已发布文章、系列封面或个人资料引用，服务端将拒绝删除。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setDeleteConfirmOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="danger"
                            disabled={deleteMutation.isPending}
                            onClick={() => {
                                if (activeMedia) deleteMutation.mutate(activeMedia.id);
                            }}
                        >
                            {deleteMutation.isPending ? '删除中…' : '确认删除'}
                        </Button>
                    </>
                }
            >
                <p className="font-code text-[13px] text-ui-sub">
                    {activeMedia?.original_filename}
                </p>
            </Modal>
        </PageContainer>
    );
}
