import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { z } from 'astro/zod';

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

const snapshotSchema = z
    .object({
        schema_version: z.literal(SCHEMA_VERSION),
        generation: z.int().nonnegative(),
        generated_at: datetime,
        posts: z.array(post),
        categories: z.array(
            z.object({ slug, name: z.string().min(1), description: z.string().nullable() }),
        ),
        tags: z.array(z.object({ slug, name: z.string().min(1) })),
        profile: profile.nullable(),
        ai_trending: z.object({ data_date: z.null(), repositories: z.tuple([]) }),
        media: z.array(z.object({ id, mime_type: z.string().min(1), width: size, height: size })),
    })
    .superRefine((data, ctx) => {
        const fail = (message: string) => ctx.addIssue({ code: 'custom', message });
        const unique = (label: string, values: string[]) => {
            if (new Set(values).size !== values.length) fail(`${label} 存在重复`);
        };
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
    });

export type Snapshot = z.infer<typeof snapshotSchema>;
export type SnapshotImage = z.infer<typeof image>;

function fail(message: string): never {
    throw new Error(`[silva] 快照校验失败：${message}`);
}

function requireEnv(name: string) {
    const value = process.env[name];
    if (!value) fail(`缺少环境变量 ${name}`);
    return value;
}

async function readSnapshot(path: string): Promise<Snapshot> {
    if (!isAbsolute(path)) fail('SILVA_SNAPSHOT 必须是绝对路径');
    const expectedHash = requireEnv('SILVA_SNAPSHOT_SHA256');
    if (!/^[0-9a-f]{64}$/.test(expectedHash)) fail('SILVA_SNAPSHOT_SHA256 必须是小写十六进制');
    const generationText = requireEnv('SILVA_SNAPSHOT_GENERATION');
    if (!/^\d+$/.test(generationText)) fail('SILVA_SNAPSHOT_GENERATION 必须是十进制整数');

    const bytes = await readFile(path);
    const actualHash = createHash('sha256').update(bytes).digest('hex');
    if (actualHash !== expectedHash) fail(`SHA-256 不匹配（实际 ${actualHash}）`);

    let raw: unknown;
    try {
        raw = JSON.parse(bytes.toString('utf8'));
    } catch (error) {
        fail(`JSON 无法解析：${(error as Error).message}`);
    }
    const head = z.object({ schema_version: z.unknown(), generation: z.unknown() }).safeParse(raw);
    if (!head.success) fail('顶层必须是对象');
    if (head.data.schema_version !== SCHEMA_VERSION)
        fail(
            `schema_version 为 ${String(head.data.schema_version)}，构建器仅支持 ${SCHEMA_VERSION}`,
        );
    if (head.data.generation !== Number(generationText))
        fail(`generation 为 ${String(head.data.generation)}，期望 ${generationText}`);

    const parsed = snapshotSchema.safeParse(raw);
    if (!parsed.success) fail(`结构不符合契约\n${z.prettifyError(parsed.error)}`);
    return parsed.data;
}

let cached: Promise<Snapshot | undefined> | undefined;

/** 未设置 SILVA_SNAPSHOT 时返回 undefined（使用示例内容）；校验失败时抛出。 */
export function loadSnapshot() {
    const path = process.env.SILVA_SNAPSHOT;
    cached ??= path ? readSnapshot(path) : Promise.resolve(undefined);
    return cached;
}
