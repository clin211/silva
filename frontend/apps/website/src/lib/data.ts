import type { MarkdownHeading } from 'astro';
import { getCollection, getEntry, render, type CollectionEntry } from 'astro:content';
import * as taxonomy from '../data/taxonomy';
import * as sampleTrending from '../data/trending';
import { highlightCode } from './highlight';
import { loadSnapshot, type Snapshot, type SnapshotImage } from './snapshot';
import { htmlText, markdownText, paragraphs, readingStats, summarize } from './text';

// 页面只通过本模块读取数据：设置 SILVA_SNAPSHOT 时读取已校验的快照，否则读取仓库内示例内容。

export interface Category {
    slug: string;
    name: string;
    description?: string;
    color?: string;
}
export interface Tag {
    slug: string;
    name: string;
}
export interface Image {
    src: string;
    width: number;
    height: number;
    alt: string;
}
export const coverColors = ['teal', 'deep', 'sun', 'coral', 'mist', 'ink'] as const;
export type Cover =
    | ({ kind: 'image' } & Image)
    | { kind: 'placeholder'; color: (typeof coverColors)[number]; mark: string };
/** 正文：示例内容为 Markdown 条目，快照为消毒后的 HTML。 */
export type Body = { html: string } | { entry: CollectionEntry<'posts' | 'profile'> };

export interface Post {
    slug: string;
    title: string;
    summary: string;
    seoTitle?: string;
    seoDescription?: string;
    /** 日期或带偏移的 RFC 3339 时间，前 10 位为展示日期。 */
    published: string;
    updated?: string;
    /** 第一个为主分类，可能为空。 */
    categories: Category[];
    tags: Tag[];
    featured: boolean;
    cover?: Cover;
    headings: MarkdownHeading[];
    reading: { count: number; minutes: number };
    body: Body;
}

export interface Profile {
    name: string;
    initials: string;
    title?: string;
    location?: string;
    summary?: string;
    avatar?: Image;
    focus: string[];
    links: { label: string; url: string }[];
    experience: {
        period?: string;
        role: string;
        organization?: string;
        description?: string;
        descriptionHtml?: string;
    }[];
    skills: { group: string; items: string[] }[];
    bio?: Body;
}

const mediaUrl = (id: string) => `/api/v1/public/media/${id}`;
const toImage = (image: SnapshotImage): Image => ({
    src: mediaUrl(image.media_id),
    width: image.width,
    height: image.height,
    alt: image.alt ?? '',
});

const month = (date: string) => date.slice(0, 7).replace('-', '.');

/** 只公开 http、https 与 mailto 链接，其他协议（如 javascript:）不渲染。 */
function safeLink(url: string) {
    try {
        return ['http:', 'https:', 'mailto:'].includes(new URL(url).protocol);
    } catch {
        return false;
    }
}

function period(started: string | null, ended: string | null) {
    if (started) return `${month(started)}${ended ? ` – ${month(ended)}` : ' 起'}`;
    return ended ? `至 ${month(ended)}` : undefined;
}

function hash(value: string) {
    let result = 0;
    for (const char of value) result = (result * 31 + char.codePointAt(0)!) >>> 0;
    return result;
}

const sampleCategories = (): Category[] =>
    taxonomy.categorySlugs.map((slug) => ({ slug, ...taxonomy.categories[slug] }));
const sampleTags = (): Tag[] =>
    taxonomy.tagSlugs.map((slug) => ({ slug, name: taxonomy.tags[slug] }));

async function samplePosts(): Promise<Post[]> {
    const categories = new Map(sampleCategories().map((item) => [item.slug, item]));
    const entries = await getCollection('posts');
    entries.sort(
        (a, b) => b.data.published.localeCompare(a.data.published) || a.id.localeCompare(b.id),
    );
    return Promise.all(
        entries.map(async (entry) => {
            const { data } = entry;
            return {
                slug: entry.id,
                title: data.title,
                summary: data.summary,
                published: data.published,
                updated: data.updated !== data.published ? data.updated : undefined,
                categories: [categories.get(data.category)!],
                tags: data.tags.map((slug) => ({ slug, name: taxonomy.tags[slug] })),
                featured: data.featured,
                cover: data.cover && { kind: 'placeholder', ...data.cover },
                headings: (await render(entry)).headings,
                reading: readingStats(markdownText(entry.body)),
                body: { entry },
            } satisfies Post;
        }),
    );
}

async function snapshotPosts(snapshot: Snapshot): Promise<Post[]> {
    const categories = new Map(
        snapshotCategories(snapshot).map((item) => [item.slug, item] as const),
    );
    const tags = new Map(snapshot.tags.map((item) => [item.slug, item] as const));
    return Promise.all(
        snapshot.posts.map(async (post) => {
            const text = htmlText(post.html);
            return {
                slug: post.slug,
                title: post.title,
                summary: post.excerpt ?? summarize(htmlText(paragraphs(post.html))),
                seoTitle: post.seo_title ?? undefined,
                seoDescription: post.seo_description ?? undefined,
                published: post.published_at,
                updated: post.updated_at !== post.published_at ? post.updated_at : undefined,
                categories: post.categories.map((slug) => categories.get(slug)!),
                tags: post.tags.map((slug) => tags.get(slug)!),
                featured: post.featured,
                cover: post.cover
                    ? { kind: 'image', ...toImage(post.cover) }
                    : {
                          kind: 'placeholder',
                          color: coverColors[hash(post.slug) % coverColors.length],
                          mark: post.tags[0] ?? post.categories[0] ?? post.slug.split('-')[0],
                      },
                headings: post.headings,
                reading: readingStats(text),
                body: { html: await highlightCode(post.html) },
            } satisfies Post;
        }),
    );
}

const snapshotCategories = (snapshot: Snapshot): Category[] =>
    snapshot.categories.map(({ slug, name, description }) => ({
        slug,
        name,
        description: description ?? undefined,
    }));

let posts: Promise<Post[]> | undefined;

/** 按展示顺序（发布时间降序）返回全部文章。 */
export function listPosts() {
    posts ??= loadSnapshot().then((snapshot) =>
        snapshot ? snapshotPosts(snapshot) : samplePosts(),
    );
    return posts;
}

export async function listCategories() {
    const snapshot = await loadSnapshot();
    return snapshot ? snapshotCategories(snapshot) : sampleCategories();
}

export async function listTags(): Promise<Tag[]> {
    const snapshot = await loadSnapshot();
    return snapshot ? snapshot.tags.map(({ slug, name }) => ({ slug, name })) : sampleTags();
}

/** 快照模式下返回 generation，示例模式返回 undefined。 */
export async function getGeneration() {
    return (await loadSnapshot())?.generation;
}

export async function getProfile(): Promise<Profile | undefined> {
    const snapshot = await loadSnapshot();
    if (!snapshot) {
        const entry = await getEntry('profile', 'owner');
        if (!entry) return undefined;
        const { data } = entry;
        return {
            ...data,
            bio: entry.body?.trim() ? { entry } : undefined,
        };
    }
    const profile = snapshot.profile;
    if (!profile) return undefined;
    const bio = profile.bio_html && htmlText(profile.bio_html);
    const skills = new Map<string, string[]>();
    for (const { name, category } of profile.skills) {
        const group = category ?? '其他';
        skills.set(group, [...(skills.get(group) ?? []), name]);
    }
    return {
        name: profile.display_name,
        initials: Array.from(profile.display_name)[0].toUpperCase(),
        title: profile.headline ?? undefined,
        location: profile.location ?? undefined,
        summary: (bio && summarize(bio, 80)) || profile.headline || undefined,
        avatar: profile.avatar ? toImage(profile.avatar) : undefined,
        focus: [],
        links: profile.links
            .filter(({ url }) => safeLink(url))
            .map(({ platform, label, url }) => ({ label: label || platform, url })),
        experience: profile.experiences.map((item) => ({
            period: period(item.started_on, item.ended_on),
            role: item.role,
            organization: item.organization ?? undefined,
            descriptionHtml: item.description_html ?? undefined,
        })),
        skills: [...skills].map(([group, items]) => ({ group, items })),
        bio: bio ? { html: profile.bio_html! } : undefined,
    };
}

/** AI 趋势榜；快照中 data_date 为 null 时返回 null（显示「暂无数据」）。 */
export async function getTrending() {
    const snapshot = await loadSnapshot();
    return snapshot ? null : sampleTrending.loadTrending();
}

export async function getRepositoryHistory(slug: string) {
    return (await loadSnapshot()) ? [] : sampleTrending.historyFor(slug);
}
