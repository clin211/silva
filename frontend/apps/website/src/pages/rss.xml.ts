import rss from '@astrojs/rss';
import type { APIRoute } from 'astro';
import { siteOrigin } from '../lib/origin';
import { postUrl, siteDescription, siteName } from '../lib/site';

export const GET: APIRoute = async (context) =>
    rss({
        title: `${siteName} · 技术文章`,
        description: siteDescription,
        site: siteOrigin(context),
        items: (await context.locals.content.listPosts()).map((post) => ({
            title: post.title,
            pubDate: new Date(post.published),
            description: post.summary,
            link: postUrl(post.slug),
        })),
        customData: '<language>zh-CN</language>',
        trailingSlash: true,
    });
