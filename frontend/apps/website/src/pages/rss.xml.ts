import rss from '@astrojs/rss';
import type { APIRoute } from 'astro';
import { listPosts } from '../lib/data';
import { postUrl, siteDescription, siteName } from '../lib/site';

export const GET: APIRoute = async ({ site, url }) =>
    rss({
        title: `${siteName} · 技术文章`,
        description: siteDescription,
        site: site ?? url.origin,
        items: (await listPosts()).map((post) => ({
            title: post.title,
            pubDate: new Date(post.published),
            description: post.summary,
            link: postUrl(post.slug),
        })),
        customData: '<language>zh-CN</language>',
        trailingSlash: true,
    });
