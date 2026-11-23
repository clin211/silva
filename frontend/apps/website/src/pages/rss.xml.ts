import rss from '@astrojs/rss';
import type { APIRoute } from 'astro';
import { listPosts } from '../lib/content';
import { postUrl, siteDescription, siteName } from '../lib/site';

export const GET: APIRoute = async ({ site, url }) =>
    rss({
        title: `${siteName} · 技术文章`,
        description: siteDescription,
        site: site ?? url.origin,
        items: (await listPosts()).map(({ id, data }) => ({
            title: data.title,
            pubDate: new Date(data.published),
            description: data.summary,
            link: postUrl(id),
        })),
        customData: '<language>zh-CN</language>',
        trailingSlash: true,
    });
