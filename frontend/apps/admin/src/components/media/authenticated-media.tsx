import { useEffect, useRef, useState } from 'react';
import { Image as ImageIcon } from 'lucide-react';
import { fetchMediaBlob } from '@/lib/api';
import { cn } from '@/lib/cn';
import { isValidHexColor } from '@/lib/markdown';

/**
 * 通过 Bearer Token 安全读取私有媒体内容，并在组件卸载时释放 Object URL。
 */
export function AuthenticatedImage({
    mediaId,
    alt,
    className,
    fallbackClassName,
}: {
    mediaId: string | null | undefined;
    alt: string;
    className?: string;
    fallbackClassName?: string;
}) {
    const [loaded, setLoaded] = useState<{ id: string; url: string | null } | null>(null);

    useEffect(() => {
        if (!mediaId) return;
        const controller = new AbortController();
        let objectUrl: string | null = null;

        fetchMediaBlob(mediaId, controller.signal)
            .then((blob) => {
                objectUrl = URL.createObjectURL(blob);
                setLoaded({ id: mediaId, url: objectUrl });
            })
            .catch((error: unknown) => {
                if (error instanceof DOMException && error.name === 'AbortError') return;
                setLoaded({ id: mediaId, url: null });
            });

        return () => {
            controller.abort();
            if (objectUrl) {
                URL.revokeObjectURL(objectUrl);
            }
        };
    }, [mediaId]);

    const activeUrl = mediaId && loaded?.id === mediaId ? loaded.url : null;

    if (!activeUrl) {
        return (
            <div
                className={cn(
                    'grid place-items-center bg-[#EEF0F3] text-ui-sub',
                    fallbackClassName ?? className,
                )}
            >
                <ImageIcon className="h-6 w-6 opacity-45" aria-hidden="true" />
            </div>
        );
    }

    return <img src={activeUrl} alt={alt} className={className} loading="lazy" />;
}

/**
 * 渲染经过 DOMPurify 消毒的 HTML，并将其中带 `data-silva-media-id` 的图片
 * 通过认证接口 `GET /api/v1/admin/media/{id}/content` 替换为本地 blob URL。
 */
export function SafeHtmlPreview({
    html,
    emptyText = '暂无正文内容。',
    className,
}: {
    html: string;
    emptyText?: string;
    className?: string;
}) {
    const containerRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const host = containerRef.current;
        if (!host) return;

        const controller = new AbortController();
        const createdUrls: string[] = [];
        const captions = Array.from(host.querySelectorAll<HTMLElement>('[data-silva-alt-color]'));
        for (const caption of captions) {
            const color = caption.getAttribute('data-silva-alt-color');
            if (isValidHexColor(color)) {
                caption.style.color = color;
            }
        }

        const imgs = Array.from(
            host.querySelectorAll<HTMLImageElement>('img[data-silva-media-id]'),
        );

        for (const img of imgs) {
            const mediaId = img.getAttribute('data-silva-media-id');
            if (!mediaId) continue;
            fetchMediaBlob(mediaId, controller.signal)
                .then((blob) => {
                    if (controller.signal.aborted) return;
                    const blobUrl = URL.createObjectURL(blob);
                    createdUrls.push(blobUrl);
                    img.src = blobUrl;
                })
                .catch(() => {
                    if (controller.signal.aborted) return;
                    img.alt = `${img.alt || '图片'}（无法加载媒体）`;
                });
        }

        return () => {
            controller.abort();
            for (const url of createdUrls) {
                URL.revokeObjectURL(url);
            }
        };
    }, [html]);

    if (!html.trim()) {
        return <p className="text-[14px] text-ink-soft">{emptyText}</p>;
    }

    return (
        <div
            ref={containerRef}
            className={cn('prose-ink', className)}
            // HTML 已通过 DOMPurify 严格消毒
            dangerouslySetInnerHTML={{ __html: html }}
        />
    );
}
