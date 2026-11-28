import { useEffect, useRef, useState } from 'react';
import { Image as ImageIcon } from 'lucide-react';
import { fetchMediaBlob } from '@/lib/api';
import { cn } from '@/lib/cn';
import { isValidHexColor } from '@/lib/markdown';
import { applyTokenStyleToElement, getCodeLanguageLabel, tokenizeCodeAsync } from '@/lib/shiki';

const CODE_LANG_CLASS_REGEX = /(?:^|\s)language-([a-zA-Z0-9_+.#-]+)(?=\s|$)/;

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

        // 异步使用 Shiki 为预览区代码块着色（通过 DOM API 构造 span，避免破坏消毒边界）
        const codeBlocks = Array.from(host.querySelectorAll<HTMLElement>('pre > code'));
        for (const codeEl of codeBlocks) {
            const pre = codeEl.parentElement;
            if (!pre) continue;
            const match = CODE_LANG_CLASS_REGEX.exec(codeEl.className);
            const rawLang = match?.[1] ?? '';
            const langLabel = getCodeLanguageLabel(rawLang);

            // 幂等包裹代码块容器与语言标题栏，避免横向滚动导致标题栏错位，兼容 StrictMode 二次执行
            let wrapper = pre.parentElement;
            if (!wrapper?.classList.contains('silva-code-block')) {
                wrapper = document.createElement('div');
                wrapper.className = 'silva-code-block';
                const header = document.createElement('div');
                header.className = 'silva-code-block-header';
                const badge = document.createElement('span');
                badge.className = 'font-code text-[11.5px] font-medium text-[#8b949e]';
                badge.textContent = langLabel;
                header.append(badge);
                pre.classList.add('silva-code-block-pre');
                pre.parentNode?.insertBefore(wrapper, pre);
                wrapper.append(header, pre);
            }
            wrapper.setAttribute('data-language', rawLang || 'plaintext');

            const rawCode = codeEl.textContent ?? '';
            if (!rawLang || !rawCode) continue;

            void tokenizeCodeAsync(rawCode, rawLang).then((lines) => {
                if (controller.signal.aborted || !lines) return;
                const fragment = document.createDocumentFragment();
                for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
                    const tokens = lines[lineIndex] ?? [];
                    for (const token of tokens) {
                        const span = document.createElement('span');
                        span.textContent = token.content;
                        applyTokenStyleToElement(span, token);
                        fragment.append(span);
                    }
                    if (lineIndex < lines.length - 1) {
                        fragment.append(document.createTextNode('\n'));
                    }
                }
                codeEl.replaceChildren(fragment);
                pre.classList.add('silva-shiki');
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
