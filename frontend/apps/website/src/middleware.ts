import { defineMiddleware } from 'astro:middleware';
import { createSiteData, type SiteData } from './lib/data';
import { fetchPublishedSnapshot } from './lib/snapshot';

let cached: { etag: string; content: SiteData } | undefined;

async function currentContent() {
    const previous = cached;
    const updated = await fetchPublishedSnapshot(previous?.etag);
    if (!updated && previous) return previous.content;
    if (!updated) throw new Error('上游未提供公开站点版本');
    const content = createSiteData(updated.snapshot);
    if (updated.etag) cached = { etag: updated.etag, content };
    return content;
}

export const onRequest = defineMiddleware(async (context, next) => {
    const { pathname, search } = context.url;
    if (pathname.startsWith('/api/v1/public/')) return next();
    if (/^\/(?:about|blog|series|categories|tags|ai-trending)(?:\/[^/.]+)?$/.test(pathname)) {
        return context.redirect(`${pathname}/${search}`, 301);
    }
    // Rewrites (including a missing slug → 404) keep the same request's version.
    if (!context.locals.content) {
        try {
            context.locals.content = await currentContent();
        } catch (error) {
            console.error('[silva] 无法读取公开站点数据', error);
            return new Response('站点暂时无法读取，请稍后重试。', {
                status: 503,
                headers: {
                    'Cache-Control': 'no-store',
                    'Content-Type': 'text/plain; charset=utf-8',
                },
            });
        }
    }
    const response = await next();
    response.headers.set('Cache-Control', 'no-store');
    response.headers.set('X-Site-Generation', String(await context.locals.content.getGeneration()));
    return response;
});
