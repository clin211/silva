import type { APIRoute } from 'astro';
import { publicApiOrigin } from '../lib/api-origin';

/** Standalone preview serves the same public media URLs as the production gateway. */
export const GET: APIRoute = async ({ request, url }) => {
    const upstream = publicApiOrigin();
    // Astro normalizes directory routes to a trailing slash; Rust API resources use exact paths.
    upstream.pathname = url.pathname.replace(/\/$/, '');
    upstream.search = url.search;
    const headers = new Headers();
    for (const key of ['if-none-match', 'x-expected-generation']) {
        const value = request.headers.get(key);
        if (value) headers.set(key, value);
    }
    headers.set('Accept-Encoding', 'identity');
    try {
        return await fetch(upstream, {
            method: request.method,
            headers,
            redirect: 'manual',
            signal: request.signal,
        });
    } catch {
        return new Response('公开接口暂不可用', {
            status: 502,
            headers: { 'Cache-Control': 'no-store' },
        });
    }
};
export const HEAD = GET;
