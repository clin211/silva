import { z } from 'astro/zod';
import { publicApiOrigin } from './api-origin.ts';

// 快照契约见 server/docs/contracts/site-snapshot.md
export const SCHEMA_VERSION = 1;

const id = z.uuid();
const slug = z
    .string()
    .max(100)
    .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/);
const datetime = z.iso.datetime({ offset: true });
const size = z.int().positive();
const image = z.object({
    media_id: id,
    mime_type: z.string().min(1),
    width: size,
    height: size,
    alt: z.string().nullable(),
});

const post = z.object({
    id,
    revision_id: id,
    slug,
    title: z.string().min(1),
    excerpt: z.string().nullable(),
    html: z.string(),
    headings: z.array(
        z.object({ depth: z.int().min(2).max(4), slug: z.string().min(1), text: z.string() }),
    ),
    categories: z.array(slug),
    tags: z.array(slug),
    featured: z.boolean(),
    cover: image.nullable(),
    seo_title: z.string().nullable(),
    seo_description: z.string().nullable(),
    published_at: datetime,
    updated_at: datetime,
});

const profile = z.object({
    display_name: z.string().min(1),
    headline: z.string().nullable(),
    bio_html: z.string().nullable(),
    location: z.string().nullable(),
    avatar: image.nullable(),
    experiences: z.array(
        z.object({
            organization: z.string().nullable(),
            role: z.string().min(1),
            description_html: z.string().nullable(),
            started_on: z.iso.date().nullable(),
            ended_on: z.iso.date().nullable(),
        }),
    ),
    skills: z.array(
        z.object({
            name: z.string().min(1),
            category: z.string().nullable(),
            proficiency: z.string().nullable(),
        }),
    ),
    links: z.array(
        z.object({
            platform: z.string(),
            label: z.string().nullable(),
            // 服务端不限制链接协议；构建时只渲染 http、https 与 mailto 链接。
            url: z.string().min(1),
        }),
    ),
});

const seriesCategory = z.object({
    slug,
    name: z.string().min(1),
    description: z.string().nullable(),
});

const seriesItem = z.object({
    post_slug: slug,
    display_title: z.string().min(1).nullable(),
});

const seriesSection = z.object({
    title: z.string().min(1),
    description: z.string().nullable(),
    items: z.array(seriesItem),
});

const series = z.object({
    id,
    slug,
    title: z.string().min(1),
    description: z.string().nullable(),
    category: seriesCategory.nullable(),
    completion_status: z.enum(['ongoing', 'completed']),
    featured: z.boolean(),
    cover: image.nullable(),
    seo_title: z.string().nullable(),
    seo_description: z.string().nullable(),
    published_at: datetime,
    updated_at: datetime,
    items: z.array(seriesItem),
    sections: z.array(seriesSection),
});

const snapshotSchema = z
    .object({
        schema_version: z.literal(SCHEMA_VERSION),
        generation: z.int().nonnegative(),
        generated_at: datetime,
        posts: z.array(post),
        categories: z.array(seriesCategory),
        tags: z.array(z.object({ slug, name: z.string().min(1) })),
        profile: profile.nullable(),
        ai_trending: z.object({ data_date: z.null(), repositories: z.tuple([]) }),
        media: z.array(z.object({ id, mime_type: z.string().min(1), width: size, height: size })),
        series: z.array(series).default([]),
    })
    .superRefine((data, ctx) => {
        const fail = (message: string) => ctx.addIssue({ code: 'custom', message });
        const unique = (label: string, values: string[]) => {
            if (new Set(values).size !== values.length) fail(`${label} 存在重复`);
        };
        const postSlugs = new Set(data.posts.map((item) => item.slug));
        const categories = new Set(data.categories.map((item) => item.slug));
        const tags = new Set(data.tags.map((item) => item.slug));
        const media = new Set(data.media.map((item) => item.id));
        unique(
            'posts.slug',
            data.posts.map((item) => item.slug),
        );
        unique(
            'categories.slug',
            data.categories.map((item) => item.slug),
        );
        unique(
            'tags.slug',
            data.tags.map((item) => item.slug),
        );
        unique(
            'series.id',
            data.series.map((item) => item.id),
        );
        unique(
            'series.slug',
            data.series.map((item) => item.slug),
        );
        for (const item of data.posts) {
            for (const value of item.categories)
                if (!categories.has(value)) fail(`文章 ${item.slug} 引用了未知分类 ${value}`);
            for (const value of item.tags)
                if (!tags.has(value)) fail(`文章 ${item.slug} 引用了未知标签 ${value}`);
            if (item.cover && !media.has(item.cover.media_id))
                fail(`文章 ${item.slug} 的封面不在 media 中`);
        }
        if (data.profile?.avatar && !media.has(data.profile.avatar.media_id))
            fail('头像不在 media 中');
        const seenSeriesPosts = new Set<string>();
        const checkSeriesItem = (seriesSlug: string, postSlug: string) => {
            if (!postSlugs.has(postSlug)) fail(`系列 ${seriesSlug} 引用了未知文章 ${postSlug}`);
            if (seenSeriesPosts.has(postSlug)) fail(`文章 ${postSlug} 在系列目录中重复出现`);
            seenSeriesPosts.add(postSlug);
        };
        for (const entry of data.series) {
            if (entry.cover && !media.has(entry.cover.media_id))
                fail(`系列 ${entry.slug} 的封面不在 media 中`);
            let total = 0;
            for (const item of entry.items) {
                checkSeriesItem(entry.slug, item.post_slug);
                total += 1;
            }
            for (const section of entry.sections) {
                if (section.items.length === 0)
                    fail(`系列 ${entry.slug} 包含空篇章「${section.title}」`);
                for (const item of section.items) {
                    checkSeriesItem(entry.slug, item.post_slug);
                    total += 1;
                }
            }
            if (total === 0) fail(`系列 ${entry.slug} 不包含任何公开文章`);
        }
    });

export type Snapshot = z.infer<typeof snapshotSchema>;
export type SnapshotImage = z.infer<typeof image>;
export type SnapshotSeries = z.infer<typeof series>;

/** 同一响应只读取一次公开投影；上游异常时抛出错误。 */
export async function fetchPublishedSnapshot(etag?: string) {
    const url = publicApiOrigin();
    url.pathname = process.env.SILVA_SITE_SNAPSHOT_PATH || '/api/v1/public/site-snapshot';
    const response = await fetch(url, {
        cache: 'no-store',
        headers: etag ? { 'If-None-Match': etag } : {},
        signal: AbortSignal.timeout(10000),
    });
    if (response.status === 304 && etag) return undefined;
    if (!response.ok) throw new Error(`公开站点接口返回 HTTP ${response.status}`);
    const { data } = await response.json();
    return { snapshot: snapshotSchema.parse(data), etag: response.headers.get('ETag') };
}
