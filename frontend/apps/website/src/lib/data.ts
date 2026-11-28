import type { MarkdownHeading } from 'astro';
import { highlightCode } from './highlight';
import type { Snapshot, SnapshotImage } from './snapshot';
import { htmlText, paragraphs, readingStats, summarize } from './text';
import type { Sample, TrendingSnapshot } from './trending';

// 每个公开版本独立派生展示数据，草稿不会进入页面。

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
/** 正文：服务端消毒后的 HTML。 */
export interface Body {
    renderHtml: () => string | Promise<string>;
}

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

export type SeriesCompletionStatus = 'ongoing' | 'completed';

export interface SeriesEntry {
    /** 全系列公开阅读序号（从 1 起，根条目在前，随后按篇章逐组展开）。 */
    index: number;
    post: Post;
    displayTitle: string;
    sectionTitle?: string;
}

export interface SeriesSection {
    title: string;
    description?: string;
    items: SeriesEntry[];
}

export interface Series {
    id: string;
    slug: string;
    title: string;
    description?: string;
    category?: Category;
    completionStatus: SeriesCompletionStatus;
    featured: boolean;
    cover: Cover;
    seoTitle?: string;
    seoDescription?: string;
    published: string;
    updated: string;
    rootItems: SeriesEntry[];
    sections: SeriesSection[];
    entries: SeriesEntry[];
    postCount: number;
    totalMinutes: number;
}

export interface PostSeriesContext {
    series: Series;
    entry: SeriesEntry;
    index: number;
    total: number;
    prev?: SeriesEntry;
    next?: SeriesEntry;
}

export interface Profile {
    name: string;
    initials: string;
    title?: string;
    location?: string;
    summary?: string;
    avatar?: Image;
    links: { label: string; url: string }[];
    experience: {
        period?: string;
        role: string;
        organization?: string;
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

const categoryChipColors = [
    'chip-teal',
    'chip-indigo',
    'chip-coral',
    'chip-sun',
    'chip-done',
] as const;

export function categoryColorForSlug(slug: string, explicitColor?: string) {
    return explicitColor ?? categoryChipColors[hash(slug) % categoryChipColors.length];
}

export interface SeriesCategoryGroup {
    /** 分类 slug；未分类系列使用 '__uncategorized__'。 */
    key: string;
    category?: Category;
    label: string;
    description?: string;
    color: string;
    series: Series[];
    totalPosts: number;
    totalMinutes: number;
}

export function groupSeriesByCategory(
    seriesList: Series[],
    categories: Category[] = [],
): SeriesCategoryGroup[] {
    const bySlug = new Map<string, Series[]>();
    const metaBySlug = new Map<string, Category>();
    for (const cat of categories) metaBySlug.set(cat.slug, cat);

    const uncategorized: Series[] = [];
    const seenOrder: string[] = [];

    for (const item of seriesList) {
        if (!item.category) {
            uncategorized.push(item);
            continue;
        }
        const slug = item.category.slug;
        if (!metaBySlug.has(slug)) metaBySlug.set(slug, item.category);
        const list = bySlug.get(slug);
        if (list) list.push(item);
        else {
            bySlug.set(slug, [item]);
            seenOrder.push(slug);
        }
    }

    const orderedSlugs: string[] = [];
    for (const cat of categories) {
        if (bySlug.has(cat.slug)) orderedSlugs.push(cat.slug);
    }
    for (const slug of seenOrder) {
        if (!orderedSlugs.includes(slug)) orderedSlugs.push(slug);
    }

    const groups: SeriesCategoryGroup[] = orderedSlugs.map((slug) => {
        const cat = metaBySlug.get(slug)!;
        const items = bySlug.get(slug)!;
        return {
            key: slug,
            category: cat,
            label: cat.name,
            description: cat.description,
            color: categoryColorForSlug(slug, cat.color),
            series: items,
            totalPosts: items.reduce((sum, s) => sum + s.postCount, 0),
            totalMinutes: items.reduce((sum, s) => sum + s.totalMinutes, 0),
        };
    });

    if (uncategorized.length > 0) {
        groups.push({
            key: '__uncategorized__',
            label: '其他专栏',
            description: '暂未归入主分类的专题与系列笔记。',
            color: 'chip-indigo',
            series: uncategorized,
            totalPosts: uncategorized.reduce((sum, s) => sum + s.postCount, 0),
            totalMinutes: uncategorized.reduce((sum, s) => sum + s.totalMinutes, 0),
        });
    }

    return groups;
}

export interface SharedSeriesRecommendation {
    series: Series;
    reason: string;
    score: number;
}

export interface SeriesDomainGuide extends SeriesCategoryGroup {
    /** 领域下存在多个专栏时的推荐起点（优先精选，否则取首个）。 */
    start?: Series;
    /** 可按需并行选读的核心专栏列表。 */
    parallel: Series[];
    /** 跨领域公用延伸专栏（主归属在其他分类，但文章分类/标签与本领域高度相关）。 */
    shared: SharedSeriesRecommendation[];
}

const genericAffinityTagSlugs = new Set([
    'learning',
    'learning-path',
    'roadmap',
    'dev-env',
    'tech-selection',
    'interview',
]);
const genericAffinityTagNames = new Set(['学习路线', '持续学习', '开发环境', '技术选型', '面试']);

function seriesTagMap(series: Series) {
    const tags = new Map<string, { slug: string; name: string; count: number }>();
    for (const entry of series.entries) {
        for (const tag of entry.post.tags) {
            const existing = tags.get(tag.slug);
            if (existing) existing.count += 1;
            else tags.set(tag.slug, { slug: tag.slug, name: tag.name, count: 1 });
        }
    }
    return tags;
}

function seriesArticleCategorySlugs(series: Series) {
    const slugs = new Set<string>();
    for (const entry of series.entries) {
        for (const cat of entry.post.categories) slugs.add(cat.slug);
    }
    return slugs;
}

/**
 * 为每个拥有公开专栏的技术领域派生「建议起点 + 并行核心模块 + 跨领域公用延伸」。
 * 公用延伸依据其他分类专栏中文章的分类与标签交集自动推导，无需额外配置。
 */
export function buildSeriesDomainGuides(
    seriesList: Series[],
    categories: Category[] = [],
): SeriesDomainGuide[] {
    const groups = groupSeriesByCategory(seriesList, categories);
    const tagCache = new Map<string, Map<string, { slug: string; name: string; count: number }>>();
    const postCatCache = new Map<string, Set<string>>();

    for (const s of seriesList) {
        tagCache.set(s.slug, seriesTagMap(s));
        postCatCache.set(s.slug, seriesArticleCategorySlugs(s));
    }

    return groups.map((group) => {
        let start: Series | undefined;
        let parallel: Series[];
        if (group.series.length >= 2) {
            start = group.series.find((s) => s.featured) ?? group.series[0];
            parallel = group.series.filter((s) => s !== start);
        } else {
            start = undefined;
            parallel = [...group.series];
        }

        // 收集当前领域核心专栏的高信号技术标签
        const domainTagCounts = new Map<string, { name: string; count: number }>();
        for (const s of group.series) {
            for (const [slug, info] of tagCache.get(s.slug) ?? []) {
                if (genericAffinityTagSlugs.has(slug) || genericAffinityTagNames.has(info.name)) {
                    continue;
                }
                const prev = domainTagCounts.get(slug);
                if (prev) prev.count += info.count;
                else domainTagCounts.set(slug, { name: info.name, count: info.count });
            }
        }

        const groupSlugLower = group.key.toLowerCase();
        const groupNameLower = group.label.toLowerCase();
        const shared: SharedSeriesRecommendation[] = [];

        if (group.category) {
            for (const candidate of seriesList) {
                if ((candidate.category?.slug ?? '__uncategorized__') === group.key) continue;

                const candPostCats = postCatCache.get(candidate.slug)!;
                const candTags = tagCache.get(candidate.slug)!;

                const hasDirectCategory = candPostCats.has(group.key);
                let directTagName: string | undefined;
                for (const [slug, info] of candTags) {
                    if (
                        slug.toLowerCase() === groupSlugLower ||
                        info.name.toLowerCase() === groupNameLower
                    ) {
                        directTagName = info.name;
                        break;
                    }
                }

                const overlappedTags: { name: string; weight: number }[] = [];
                for (const [slug, info] of candTags) {
                    const domTag = domainTagCounts.get(slug);
                    if (domTag) {
                        overlappedTags.push({
                            name: info.name,
                            weight: info.count + Math.min(domTag.count, 5),
                        });
                    }
                }
                overlappedTags.sort(
                    (a, b) => b.weight - a.weight || a.name.localeCompare(b.name, 'zh-CN'),
                );

                let score = 0;
                let reason = '';
                if (hasDirectCategory && directTagName) {
                    score = 120 + overlappedTags.length * 4;
                    reason = `含「${group.label}」实践章节，可直接结合本领域学习`;
                } else if (hasDirectCategory) {
                    score = 100 + overlappedTags.length * 4;
                    reason = `内含归属「${group.label}」的相关实践文章`;
                } else if (directTagName) {
                    score = 80 + overlappedTags.length * 4;
                    reason = `含「${directTagName}」主题实践章节`;
                } else if (overlappedTags.length > 0) {
                    const topTags = overlappedTags.slice(0, 3).map((t) => t.name);
                    score = 20 + overlappedTags.reduce((sum, t) => sum + t.weight, 0);
                    reason = `共用主题：${topTags.join(' · ')}`;
                }

                if (score > 0) {
                    shared.push({ series: candidate, reason, score });
                }
            }
        }

        shared.sort(
            (a, b) =>
                b.score - a.score ||
                b.series.postCount - a.series.postCount ||
                b.series.updated.localeCompare(a.series.updated),
        );

        return {
            ...group,
            start,
            parallel,
            shared: shared.slice(0, 8),
        };
    });
}

function publishedBody(html: string): Body {
    let highlighted: Promise<string> | undefined;
    return { renderHtml: () => (highlighted ??= highlightCode(html)) };
}

function snapshotPosts(snapshot: Snapshot): Post[] {
    const categories = new Map(
        snapshotCategories(snapshot).map((item) => [item.slug, item] as const),
    );
    const tags = new Map(snapshot.tags.map((item) => [item.slug, item] as const));
    return snapshot.posts.map((post) => {
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
            body: publishedBody(post.html),
        } satisfies Post;
    });
}

const snapshotCategories = (snapshot: Snapshot): Category[] => {
    const merged = new Map<string, Category>();
    for (const { slug, name, description } of snapshot.categories) {
        merged.set(slug, {
            slug,
            name,
            description: description ?? undefined,
            color: categoryColorForSlug(slug),
        });
    }
    const extra: Category[] = [];
    for (const item of snapshot.series) {
        if (!item.category || merged.has(item.category.slug)) continue;
        const cat: Category = {
            slug: item.category.slug,
            name: item.category.name,
            description: item.category.description ?? undefined,
            color: categoryColorForSlug(item.category.slug),
        };
        merged.set(cat.slug, cat);
        extra.push(cat);
    }
    extra.sort((a, b) => a.name.localeCompare(b.name, 'zh-CN') || a.slug.localeCompare(b.slug));
    const base = snapshot.categories.map(({ slug }) => merged.get(slug)!);
    return [...base, ...extra];
};

function buildSeriesObject(params: {
    id: string;
    slug: string;
    title: string;
    description?: string;
    category?: Category;
    completionStatus: SeriesCompletionStatus;
    featured: boolean;
    cover: Cover;
    seoTitle?: string;
    seoDescription?: string;
    published: string;
    updated: string;
    rawRootItems: { postSlug: string; displayTitle?: string | null }[];
    rawSections: {
        title: string;
        description?: string | null;
        items: { postSlug: string; displayTitle?: string | null }[];
    }[];
    postsBySlug: Map<string, Post>;
}): Series | undefined {
    let cursor = 0;
    const entries: SeriesEntry[] = [];
    const rootItems: SeriesEntry[] = [];
    for (const raw of params.rawRootItems) {
        const post = params.postsBySlug.get(raw.postSlug);
        if (!post) continue;
        cursor += 1;
        const entry: SeriesEntry = {
            index: cursor,
            post,
            displayTitle: raw.displayTitle?.trim() || post.title,
        };
        rootItems.push(entry);
        entries.push(entry);
    }
    const sections: SeriesSection[] = [];
    for (const rawSection of params.rawSections) {
        const sectionItems: SeriesEntry[] = [];
        for (const raw of rawSection.items) {
            const post = params.postsBySlug.get(raw.postSlug);
            if (!post) continue;
            cursor += 1;
            const entry: SeriesEntry = {
                index: cursor,
                post,
                displayTitle: raw.displayTitle?.trim() || post.title,
                sectionTitle: rawSection.title,
            };
            sectionItems.push(entry);
            entries.push(entry);
        }
        if (sectionItems.length > 0) {
            sections.push({
                title: rawSection.title,
                description: rawSection.description ?? undefined,
                items: sectionItems,
            });
        }
    }
    if (entries.length === 0) return undefined;
    const totalMinutes = entries.reduce((sum, item) => sum + item.post.reading.minutes, 0);
    return {
        id: params.id,
        slug: params.slug,
        title: params.title,
        description: params.description,
        category: params.category,
        completionStatus: params.completionStatus,
        featured: params.featured,
        cover: params.cover,
        seoTitle: params.seoTitle,
        seoDescription: params.seoDescription,
        published: params.published,
        updated: params.updated,
        rootItems,
        sections,
        entries,
        postCount: entries.length,
        totalMinutes,
    };
}

function defaultSeriesCover(slug: string, category?: Category): Cover {
    return {
        kind: 'placeholder',
        color: coverColors[hash(slug) % coverColors.length],
        mark: category?.slug ?? slug.split('-')[0] ?? 'SER',
    };
}

function snapshotSeries(snapshot: Snapshot, postsList: Post[]): Series[] {
    const postsBySlug = new Map(postsList.map((post) => [post.slug, post] as const));
    const list: Series[] = [];
    for (const item of snapshot.series) {
        const category: Category | undefined = item.category
            ? {
                  slug: item.category.slug,
                  name: item.category.name,
                  description: item.category.description ?? undefined,
                  color: categoryColorForSlug(item.category.slug),
              }
            : undefined;
        const built = buildSeriesObject({
            id: item.id,
            slug: item.slug,
            title: item.title,
            description: item.description ?? undefined,
            category,
            completionStatus: item.completion_status,
            featured: item.featured,
            cover: item.cover
                ? { kind: 'image', ...toImage(item.cover) }
                : defaultSeriesCover(item.slug, category),
            seoTitle: item.seo_title ?? undefined,
            seoDescription: item.seo_description ?? undefined,
            published: item.published_at,
            updated: item.updated_at,
            rawRootItems: item.items.map((entry) => ({
                postSlug: entry.post_slug,
                displayTitle: entry.display_title,
            })),
            rawSections: item.sections.map((section) => ({
                title: section.title,
                description: section.description,
                items: section.items.map((entry) => ({
                    postSlug: entry.post_slug,
                    displayTitle: entry.display_title,
                })),
            })),
            postsBySlug,
        });
        if (built) list.push(built);
    }
    return list;
}

export function createSiteData(snapshot: Snapshot) {
    let posts: Promise<Post[]> | undefined;
    let seriesCache: Promise<Series[]> | undefined;
    let postSeriesMapCache: Promise<Map<string, PostSeriesContext>> | undefined;

    /** 按展示顺序（发布时间降序）返回全部文章。 */
    function listPosts() {
        posts ??= Promise.resolve(snapshotPosts(snapshot));
        return posts;
    }

    /** 按展示顺序返回全部可见系列（仅包含至少一篇公开文章的系列）。 */
    function listSeries() {
        seriesCache ??= listPosts().then((postsList) => snapshotSeries(snapshot, postsList));
        return seriesCache;
    }

    /** 按公开阅读顺序生成一次文章 slug → 系列上下文映射（含序号、总数与跨篇章上下篇）。 */
    function getPostSeriesMap() {
        postSeriesMapCache ??= listSeries().then((seriesList) => {
            const map = new Map<string, PostSeriesContext>();
            for (const item of seriesList) {
                const total = item.entries.length;
                for (let i = 0; i < total; i += 1) {
                    const entry = item.entries[i]!;
                    map.set(entry.post.slug, {
                        series: item,
                        entry,
                        index: entry.index,
                        total,
                        prev: i > 0 ? item.entries[i - 1] : undefined,
                        next: i + 1 < total ? item.entries[i + 1] : undefined,
                    });
                }
            }
            return map;
        });
        return postSeriesMapCache;
    }

    async function listCategories() {
        const categories = snapshotCategories(snapshot);
        const posts = await listPosts();
        const series = await listSeries();
        const visible = new Set(posts.flatMap((post) => post.categories.map((item) => item.slug)));
        for (const item of series) if (item.category) visible.add(item.category.slug);
        return categories.filter((item) => visible.has(item.slug));
    }

    async function listTags(): Promise<Tag[]> {
        const visible = new Set(
            (await listPosts()).flatMap((post) => post.tags.map((item) => item.slug)),
        );
        return snapshot.tags.filter((item) => visible.has(item.slug));
    }

    async function getGeneration() {
        return snapshot.generation;
    }

    async function getProfile(): Promise<Profile | undefined> {
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
            bio: bio ? { renderHtml: () => profile.bio_html! } : undefined,
        };
    }

    /** AI 趋势榜；服务端尚未发布完整采集批次时返回 null（显示「暂无数据」）。 */
    async function getTrending(): Promise<TrendingSnapshot | null> {
        return null;
    }

    async function getRepositoryHistory(_slug: string): Promise<Sample[]> {
        return [];
    }

    return {
        listPosts,
        listSeries,
        getPostSeriesMap,
        listCategories,
        listTags,
        getGeneration,
        getProfile,
        getTrending,
        getRepositoryHistory,
    };
}

export type SiteData = ReturnType<typeof createSiteData>;
