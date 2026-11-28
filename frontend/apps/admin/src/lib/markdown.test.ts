import { describe, expect, it } from 'vitest';
import { renderSafeMarkdown, sanitizeServerRenderedHtml } from './markdown';

describe('safe markdown rendering & sanitization', () => {
    it('escapes raw HTML tags and blocks javascript: URLs', () => {
        const html = renderSafeMarkdown(
            '<script>alert(1)</script>\n\n[click](javascript:alert(2))\n\n**safe**',
        );
        expect(html).not.toContain('<script>');
        expect(html).not.toContain('javascript:');
        expect(html).toContain('<strong>safe</strong>');
    });

    it('converts media:<uuid> markdown images into data-silva-media-id markers and centered captions', () => {
        const id = '019535d9-3df7-79fb-b466-fa907fa17f9e';
        const html = renderSafeMarkdown(`![架构图](media:${id} "alt-color:#137a4d")`);
        expect(html).toContain(`data-silva-media-id="${id}"`);
        expect(html).toContain('alt="架构图"');
        expect(html).toContain('class="silva-img-caption"');
        expect(html).toContain('data-silva-alt-color="#137a4d"');

        const emptyAltHtml = renderSafeMarkdown(`![](media:${id})`);
        expect(emptyAltHtml).not.toContain('silva-img-caption');
    });

    it('rewrites /api/v1/public/media/<uuid> in server rendered_html for authenticated preview', () => {
        const id = '019535d9-3df7-79fb-b466-fa907fa17f9e';
        const html = sanitizeServerRenderedHtml(
            `<p>正文</p><span class="silva-img-wrap"><img src="/api/v1/public/media/${id}" alt="封面" onerror="alert(1)" /><span class="silva-img-caption" style="color:#137a4d">封面</span></span>`,
        );
        expect(html).toContain(`data-silva-media-id="${id}"`);
        expect(html).not.toContain('onerror');
        expect(html).not.toContain(`/api/v1/public/media/${id}`);
        expect(html).toContain('data-silva-alt-color="#137a4d"');
    });

    it('preserves fenced code block language classes through DOMPurify sanitization', () => {
        const html = renderSafeMarkdown(
            '```rust\nfn main() {}\n```\n\n```c++\nint main() { return 0; }\n```',
        );
        expect(html).toContain('class="language-rust"');
        expect(html).toContain('class="language-c++"');
    });
});
