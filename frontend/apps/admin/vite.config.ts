import { resolve } from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
    const env = loadEnv(mode, process.cwd(), '');
    const apiTarget = env.SILVA_API_TARGET || 'http://127.0.0.1:8088';

    const proxy = {
        '/api': {
            target: apiTarget,
            changeOrigin: true,
        },
        '/health': {
            target: apiTarget,
            changeOrigin: true,
        },
        '/metrics': {
            target: apiTarget,
            changeOrigin: true,
        },
    };

    return {
        plugins: [react(), tailwindcss()],
        resolve: {
            alias: {
                '@': resolve(import.meta.dirname, 'src'),
            },
        },
        build: {
            rollupOptions: {
                output: {
                    manualChunks: {
                        react: ['react', 'react-dom'],
                        tanstack: ['@tanstack/react-query', '@tanstack/react-router'],
                        tiptap: [
                            '@tiptap/core',
                            '@tiptap/extension-image',
                            '@tiptap/extensions',
                            '@tiptap/markdown',
                            '@tiptap/react',
                            '@tiptap/starter-kit',
                        ],
                        markdown: ['marked', 'dompurify'],
                    },
                },
            },
        },
        server: {
            port: 5173,
            proxy,
        },
        preview: {
            port: 4173,
            proxy,
        },
    };
});
