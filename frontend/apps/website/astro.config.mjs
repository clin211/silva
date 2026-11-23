import { unified } from '@astrojs/markdown-remark';
import react from '@astrojs/react';
import sitemap from '@astrojs/sitemap';
import { defineConfig } from 'astro/config';
import sanitize from 'rehype-sanitize';

const site = process.env.SITE_URL || 'http://localhost:4321';

export default defineConfig({
    site,
    output: 'static',
    trailingSlash: 'always',
    integrations: [
        react(),
        sitemap({ filter: (url) => !new URL(url).pathname.startsWith('/404') }),
    ],
    markdown: {
        processor: unified({ rehypePlugins: [sanitize] }),
    },
});
