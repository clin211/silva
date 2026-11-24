import { unified } from '@astrojs/markdown-remark';
import react from '@astrojs/react';
import sitemap from '@astrojs/sitemap';
import { defineConfig } from 'astro/config';
import sanitize from 'rehype-sanitize';
import { loadSnapshot } from './src/lib/snapshot.ts';

const site = process.env.SITE_URL || 'http://localhost:4321';

export default defineConfig({
    site,
    output: 'static',
    outDir: process.env.SILVA_SITE_OUT_DIR || './dist',
    trailingSlash: 'always',
    integrations: [
        react(),
        // 快照模式下在构建开始前完成校验，失败即中止
        { name: 'silva-snapshot', hooks: { 'astro:config:setup': () => loadSnapshot() } },
        sitemap({ filter: (url) => !new URL(url).pathname.startsWith('/404') }),
    ],
    markdown: {
        processor: unified({ rehypePlugins: [sanitize] }),
    },
});
