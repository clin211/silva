import type { SiteData } from '../lib/data';
import { postUrl, seriesUrl, tagUrl, categoryUrl, repoUrl } from '../lib/site';

export const pageSize = 45000;
export async function sitemapPaths(content: SiteData) {
    const [posts, series, categories, tags, trending] = await Promise.all([
        content.listPosts(),
        content.listSeries(),
        content.listCategories(),
        content.listTags(),
        content.getTrending(),
    ]);
    return [
        '/',
        '/about/',
        '/blog/',
        '/series/',
        '/ai-trending/',
        ...posts.map((post) => postUrl(post.slug)),
        ...series.map((item) => seriesUrl(item.slug)),
        ...categories.map((item) => categoryUrl(item.slug)),
        ...tags.map((item) => tagUrl(item.slug)),
        ...(trending?.repos ?? []).map((item) => repoUrl(item.slug)),
    ];
}

export function sitemapXml(index: boolean, paths: string[], origin: URL) {
    const root = index ? 'sitemapindex' : 'urlset';
    const tag = index ? 'sitemap' : 'url';
    const items = paths.map(
        (path) =>
            `<${tag}><loc>${new URL(path, origin).href.replaceAll('&', '&amp;')}</loc></${tag}>`,
    );
    return new Response(
        `<?xml version="1.0" encoding="UTF-8"?><${root} xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${items.join('')}</${root}>`,
        {
            headers: { 'Content-Type': 'application/xml; charset=utf-8' },
        },
    );
}
