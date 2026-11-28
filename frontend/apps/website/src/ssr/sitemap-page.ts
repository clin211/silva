import type { APIRoute } from 'astro';
import { siteOrigin } from '../lib/origin';
import { pageSize, sitemapPaths, sitemapXml } from './sitemap';

export const GET: APIRoute = async (context) => {
    const number = context.params.page ?? '';
    if (!/^(0|[1-9]\d*)$/.test(number)) return new Response(null, { status: 404 });
    const paths = await sitemapPaths(context.locals.content);
    const offset = Number(number) * pageSize;
    if (offset >= paths.length) return new Response(null, { status: 404 });
    return sitemapXml(false, paths.slice(offset, offset + pageSize), siteOrigin(context));
};
