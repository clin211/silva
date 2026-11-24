import { createShikiHighlighter } from '@astrojs/markdown-remark/shiki';
import { markdownConfigDefaults } from '@astrojs/markdown-remark';
import { decodeEntities } from './text';

const codeBlock = /<pre><code class="language-([\w+#.-]+)">([\s\S]*?)<\/code><\/pre>/g;

/** 构建时为快照正文中的代码块着色，与 Astro Markdown 默认主题一致，不引入外部资源。 */
export async function highlightCode(html: string) {
    const blocks = [...html.matchAll(codeBlock)];
    if (!blocks.length) return html;
    const shiki = await createShikiHighlighter({
        theme: markdownConfigDefaults.shikiConfig.theme,
    });
    const rendered = await Promise.all(
        blocks.map(([, lang, code]) => shiki.codeToHtml(decodeEntities(code), lang)),
    );
    let index = 0;
    return html.replace(codeBlock, () => rendered[index++]);
}
