import { existsSync } from 'node:fs';
import { loadEnvFile } from 'node:process';
import node from '@astrojs/node';
import react from '@astrojs/react';
import { defineConfig } from 'astro/config';

const localEnv = new URL('./.env', import.meta.url);
if (existsSync(localEnv)) loadEnvFile(localEnv);

const site = process.env.SITE_URL || 'http://localhost:4321';

export default defineConfig({
    site,
    outDir: process.env.SILVA_SITE_OUT_DIR || './dist',
    output: 'server',
    // Rust uses slashless API URLs; the SSR middleware canonicalizes HTML routes only.
    trailingSlash: 'ignore',
    adapter: node({ mode: 'standalone' }),
    integrations: [
        react(),
        {
            name: 'silva-live-routes',
            hooks: {
                'astro:config:setup'({ injectRoute }) {
                    injectRoute({
                        pattern: '/api/v1/public/[...path]',
                        entrypoint: './src/ssr/public-api.ts',
                    });
                    injectRoute({
                        pattern: '/sitemap-index.xml',
                        entrypoint: './src/ssr/sitemap-index.ts',
                    });
                    injectRoute({
                        pattern: '/sitemap-[page].xml',
                        entrypoint: './src/ssr/sitemap-page.ts',
                    });
                },
            },
        },
    ],
});
