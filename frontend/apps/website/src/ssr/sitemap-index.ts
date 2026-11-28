import type { APIRoute } from 'astro';
import { siteOrigin } from '../lib/origin';
import { pageSize, sitemapPaths, sitemapXml } from './sitemap';

export const GET: APIRoute = async (context) => {
    const paths = await sitemapPaths(context.locals.content);
    const pages = Array.from(
        { length: Math.ceil(paths.length / pageSize) },
        (_, i) => `/sitemap-${i}.xml`,
    );
    return sitemapXml(true, pages, siteOrigin(context));
};
