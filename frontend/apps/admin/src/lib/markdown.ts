import DOMPurify from 'dompurify';
import { Marked } from 'marked';

const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const PUBLIC_MEDIA_PATH_REGEX = /^\/api\/v1\/public\/media\/([0-9a-f-]{36})$/i;
const HEX_COLOR_REGEX = /^#[0-9a-f]{6}$/i;
const ALT_COLOR_TITLE_REGEX = /^alt-color:(#[0-9a-f]{6})$/i;

export const DEFAULT_IMAGE_ALT_COLOR = '#8a9993';

export const IMAGE_ALT_COLOR_PRESETS = [
    { label: '默认灰', value: DEFAULT_IMAGE_ALT_COLOR },
    { label: '深灰', value: '#4b5a54' },
    { label: '墨黑', value: '#111917' },
    { label: '森绿', value: '#137a4d' },
    { label: '琥珀', value: '#9a5b00' },
    { label: '绯红', value: '#b42318' },
] as const;

export function parseImageAltColor(title: string | null | undefined): string | null {
    if (!title) return null;
    const match = ALT_COLOR_TITLE_REGEX.exec(title.trim());
    return match?.[1] ? match[1].toLowerCase() : null;
}

export function formatImageAltColorTitle(color: string | null | undefined): string | null {
    if (!color) return null;
    const normalized = color.trim().toLowerCase();
    if (!HEX_COLOR_REGEX.test(normalized) || normalized === DEFAULT_IMAGE_ALT_COLOR) {
        return null;
    }
    return `alt-color:${normalized}`;
}

export function isValidHexColor(value: string | null | undefined): value is string {
    return typeof value === 'string' && HEX_COLOR_REGEX.test(value.trim());
}

function escapeHtml(text: string): string {
    return text
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

const markdownEngine = new Marked({
    gfm: true,
    breaks: false,
    renderer: {
        // 原始 HTML 一律按文本显示，与后端契约保持一致
        html({ raw }) {
            return `<p>${escapeHtml(raw)}</p>`;
        },
        image({ href, text, title }) {
            const trimmedAlt = (text ?? '').trim();
            const safeAlt = escapeHtml(trimmedAlt);
            const altColor = parseImageAltColor(title);
            const safeTitle = !altColor && title ? ` title="${escapeHtml(title)}"` : '';
            const trimmed = (href ?? '').trim();

            const wrapWithCaption = (imgHtml: string) => {
                if (!trimmedAlt) return imgHtml;
                const colorAttr = altColor ? ` data-silva-alt-color="${altColor}"` : '';
                return `<span class="silva-img-wrap">${imgHtml}<span class="silva-img-caption"${colorAttr}>${safeAlt}</span></span>`;
            };

            if (trimmed.startsWith('media:')) {
                const mediaId = trimmed.slice('media:'.length).trim();
                if (UUID_REGEX.test(mediaId)) {
                    return wrapWithCaption(
                        `<img data-silva-media-id="${mediaId}" alt="${safeAlt}"${safeTitle} loading="lazy" />`,
                    );
                }
                return `<span class="text-bad text-[13px]">[无效媒体引用：${escapeHtml(trimmed)}]</span>`;
            }

            if (/^https?:\/\//i.test(trimmed) || trimmed.startsWith('/')) {
                return wrapWithCaption(
                    `<img src="${escapeHtml(trimmed)}" alt="${safeAlt}"${safeTitle} loading="lazy" />`,
                );
            }
            return `<span class="text-ui-sub text-[13px]">[已拦截不安全图片地址]</span>`;
        },
        link({ href, title, tokens }) {
            const label = this.parser.parseInline(tokens);
            const trimmed = (href ?? '').trim();
            const safeTitle = title ? ` title="${escapeHtml(title)}"` : '';
            if (/^(https?:\/\/|mailto:|\/|#)/i.test(trimmed)) {
                const external = /^https?:\/\//i.test(trimmed)
                    ? ' target="_blank" rel="noopener noreferrer"'
                    : '';
                return `<a href="${escapeHtml(trimmed)}"${safeTitle}${external}>${label}</a>`;
            }
            return `<span>${label}</span>`;
        },
    },
});

const PURIFY_CONFIG = {
    ALLOWED_TAGS: [
        'h1',
        'h2',
        'h3',
        'h4',
        'h5',
        'h6',
        'p',
        'br',
        'hr',
        'blockquote',
        'pre',
        'code',
        'ul',
        'ol',
        'li',
        'strong',
        'em',
        'del',
        'a',
        'img',
        'figure',
        'figcaption',
        'table',
        'thead',
        'tbody',
        'tr',
        'th',
        'td',
        'span',
        'div',
    ],
    ALLOWED_ATTR: [
        'href',
        'title',
        'alt',
        'src',
        'class',
        'id',
        'target',
        'rel',
        'loading',
        'data-silva-media-id',
        'data-silva-alt-color',
    ],
    ALLOW_DATA_ATTR: false,
};

/**
 * 将 Markdown 安全渲染为已消毒 HTML，供编辑区与个人资料预览使用。
 */
export function renderSafeMarkdown(markdown: string): string {
    if (!markdown.trim()) return '';
    const rawHtml = markdownEngine.parse(markdown, { async: false });
    return DOMPurify.sanitize(rawHtml, PURIFY_CONFIG);
}

/**
 * 消毒服务端返回的 rendered_html，并将公开媒体 URL（/api/v1/public/media/{id}）
 * 标记为 data-silva-media-id，以便后台安全预览通过认证接口加载 blob。
 */
export function sanitizeServerRenderedHtml(html: string): string {
    if (!html.trim()) return '';
    let prepared = html;
    if (typeof DOMParser !== 'undefined') {
        const rawDoc = new DOMParser().parseFromString(html, 'text/html');
        const captions = rawDoc.querySelectorAll<HTMLElement>('span.silva-img-caption[style]');
        for (const caption of captions) {
            const rawStyle = caption.getAttribute('style') ?? '';
            const colorMatch = /^color:\s*(#[0-9a-f]{6})\s*;?$/i.exec(rawStyle.trim());
            if (colorMatch?.[1]) {
                caption.setAttribute('data-silva-alt-color', colorMatch[1].toLowerCase());
            }
            caption.removeAttribute('style');
        }
        prepared = rawDoc.body.innerHTML;
    }
    const sanitized = DOMPurify.sanitize(prepared, PURIFY_CONFIG);
    if (typeof DOMParser === 'undefined') {
        return sanitized;
    }
    const doc = new DOMParser().parseFromString(sanitized, 'text/html');
    const images = doc.querySelectorAll('img');
    for (const img of images) {
        const src = img.getAttribute('src') ?? '';
        const match = PUBLIC_MEDIA_PATH_REGEX.exec(src);
        if (match?.[1]) {
            img.setAttribute('data-silva-media-id', match[1]);
            img.removeAttribute('src');
        }
    }
    return doc.body.innerHTML;
}
