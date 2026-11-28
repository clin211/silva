import type { APIRoute } from 'astro';
import { siteOrigin } from '../lib/origin';

export const GET: APIRoute = (context) =>
    new Response(
        `User-agent: *\nAllow: /\nSitemap: ${new URL('/sitemap-index.xml', siteOrigin(context))}\n`,
        { headers: { 'Content-Type': 'text/plain; charset=utf-8' } },
    );
