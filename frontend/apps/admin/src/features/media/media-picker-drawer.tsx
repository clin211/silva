import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Upload } from 'lucide-react';
import { useRef, useState } from 'react';
import { AuthenticatedImage } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import { ErrorBanner } from '@/components/ui/feedback';
import { Drawer } from '@/components/ui/overlay';
import { StatusChip, type ChipTone } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { apiClient, unwrapResponse, uploadMediaFile } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId, getMediaRejectionMessage } from '@/lib/errors';
import { queryKeys } from '@/lib/query-keys';

const MEDIA_STATUS_META: Record<Schemas['MediaStatus'], { label: string; tone: ChipTone }> = {
    ready: { label: '已就绪', tone: 'ok' },
    pending: { label: '校验中', tone: 'warn' },
    rejected: { label: '未通过', tone: 'bad' },
};

export interface MediaPickerDrawerProps {
    open: boolean;
    onClose: () => void;
    onSelect: (media: Schemas['Media']) => void;
    confirmLabel?: string;
    title?: string;
}

export function MediaPickerDrawer({
    open,
    onClose,
    onSelect,
    confirmLabel = '插入',
    title = '选择媒体',
}: MediaPickerDrawerProps) {
    const queryClient = useQueryClient();
    const { toast } = useToast();
    const fileInputRef = useRef<HTMLInputElement>(null);
    const [page, setPage] = useState(1);
    const [selected, setSelected] = useState<Schemas['Media'] | null>(null);
    const [uploadError, setUploadError] = useState<string | null>(null);

    const mediaQuery = useQuery({
        queryKey: queryKeys.media.list({ page, per_page: 18 }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/media', {
                    params: { query: { page, per_page: 18 } },
                }),
            ),
        enabled: open,
    });

    const uploadMutation = useMutation({
        mutationFn: uploadMediaFile,
        onSuccess: (uploaded) => {
            setUploadError(null);
            setSelected(uploaded.media);
            toast(`已上传并校验通过：${uploaded.media.original_filename}`, 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.media.all });
        },
        onError: (error) => {
            setUploadError(formatErrorWithRequestId(error));
        },
    });

    const items = mediaQuery.data?.data ?? [];
    const pagination = mediaQuery.data?.meta.pagination;

    return (
        <Drawer
            open={open}
            onClose={onClose}
            title={title}
            subtitle="只有状态为「已就绪」的媒体可以被引用"
            widthClassName="sm:w-[540px]"
            footer={
                <div className="flex items-center justify-between gap-2">
                    <div className="text-[12px] text-ui-sub">
                        {pagination && pagination.total_pages > 1 ? (
                            <div className="flex items-center gap-2">
                                <button
                                    type="button"
                                    disabled={page <= 1}
                                    onClick={() => setPage((p) => Math.max(1, p - 1))}
                                    className="rounded border border-ui-line px-2 py-1 text-ui-text hover:bg-ui-bg disabled:opacity-40"
                                >
                                    上一页
                                </button>
                                <span className="tnum">
                                    {page} / {pagination.total_pages}
                                </span>
                                <button
                                    type="button"
                                    disabled={page >= pagination.total_pages}
                                    onClick={() => setPage((p) => p + 1)}
                                    className="rounded border border-ui-line px-2 py-1 text-ui-text hover:bg-ui-bg disabled:opacity-40"
                                >
                                    下一页
                                </button>
                            </div>
                        ) : null}
                    </div>
                    <div className="flex gap-2">
                        <Button variant="secondary" onClick={onClose}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={!selected || selected.status !== 'ready'}
                            onClick={() => {
                                if (!selected || selected.status !== 'ready') return;
                                onSelect(selected);
                                setSelected(null);
                                onClose();
                            }}
                        >
                            {confirmLabel}
                        </Button>
                    </div>
                </div>
            }
        >
            <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
                <span className="text-[13px] text-ui-sub">
                    支持 JPEG、PNG、WebP，单文件不超过 10 MiB。
                </span>
                <div>
                    <input
                        ref={fileInputRef}
                        type="file"
                        accept="image/jpeg,image/png,image/webp"
                        className="sr-only"
                        onChange={(event) => {
                            const file = event.target.files?.[0];
                            if (file) {
                                setUploadError(null);
                                uploadMutation.mutate(file);
                            }
                            event.target.value = '';
                        }}
                    />
                    <Button
                        variant="secondary"
                        size="sm"
                        disabled={uploadMutation.isPending}
                        onClick={() => fileInputRef.current?.click()}
                    >
                        <Upload className="h-3.5 w-3.5" />
                        {uploadMutation.isPending ? '上传校验中…' : '上传新图片'}
                    </Button>
                </div>
            </div>

            {uploadError ? (
                <div
                    role="alert"
                    className="mb-4 rounded-md border border-[#E8C3BF] bg-bad-tint px-3 py-2 text-[13px] text-bad"
                >
                    {uploadError}
                </div>
            ) : null}

            {mediaQuery.isError ? (
                <ErrorBanner error={mediaQuery.error} onRetry={() => void mediaQuery.refetch()} />
            ) : mediaQuery.isLoading ? (
                <div className="py-10 text-center text-[14px] text-ui-sub">正在加载媒体…</div>
            ) : items.length === 0 ? (
                <div className="rounded-lg border border-dashed border-ui-line py-10 text-center text-[14px] text-ui-sub">
                    还没有可用的媒体，点击右上角上传图片。
                </div>
            ) : (
                <ul className="grid grid-cols-2 gap-3 sm:grid-cols-3">
                    {items.map((item) => {
                        const ready = item.status === 'ready';
                        const isSelected = selected?.id === item.id;
                        const statusMeta = MEDIA_STATUS_META[item.status];
                        return (
                            <li key={item.id}>
                                <button
                                    type="button"
                                    disabled={!ready}
                                    aria-pressed={isSelected}
                                    onClick={() => setSelected(item)}
                                    className={cn(
                                        'w-full rounded-lg border p-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-65',
                                        isSelected
                                            ? 'border-accent ring-2 ring-accent/25'
                                            : 'border-ui-line enabled:hover:border-accent/60',
                                    )}
                                >
                                    <div className="aspect-[4/3] overflow-hidden rounded border border-ui-line bg-[#EEF0F3]">
                                        {ready ? (
                                            <AuthenticatedImage
                                                mediaId={item.id}
                                                alt={item.alt_text || item.original_filename}
                                                className="h-full w-full object-cover"
                                            />
                                        ) : (
                                            <div className="grid h-full w-full place-items-center text-[12px] text-ui-sub">
                                                {statusMeta.label}
                                            </div>
                                        )}
                                    </div>
                                    <div className="mt-2 truncate text-[12px] font-medium text-ui-text">
                                        {item.original_filename}
                                    </div>
                                    <div className="mt-1">
                                        <StatusChip tone={statusMeta.tone}>
                                            {statusMeta.label}
                                        </StatusChip>
                                    </div>
                                    {item.rejection_reason ? (
                                        <p className="mt-1 text-[11px] text-bad">
                                            {getMediaRejectionMessage(item.rejection_reason)}
                                        </p>
                                    ) : null}
                                </button>
                            </li>
                        );
                    })}
                </ul>
            )}
        </Drawer>
    );
}
