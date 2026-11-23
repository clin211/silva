import { getCollection, getEntry, type CollectionEntry } from 'astro:content';

export type Post = CollectionEntry<'posts'>;

export async function listPosts() {
    const posts = await getCollection('posts');
    return posts.sort(
        (a, b) => b.data.published.localeCompare(a.data.published) || a.id.localeCompare(b.id),
    );
}

export function getProfile() {
    return getEntry('profile', 'owner');
}

const wordSegmenter = new Intl.Segmenter('zh-CN', { granularity: 'word' });

export function readingStats(markdown = '') {
    const text = markdown
        .replace(/^\s*(?:`{3,}|~{3,})[^\n]*\n[\s\S]*?^\s*(?:`{3,}|~{3,})\s*$/gm, '')
        .replace(/!\[([^\]]*)\]\([^)]+\)/g, '')
        .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1');
    const characters = text.match(/\p{Script=Han}/gu)?.length ?? 0;
    const words = Array.from(wordSegmenter.segment(text)).filter(
        ({ segment, isWordLike }) => isWordLike && !/\p{Script=Han}/u.test(segment),
    ).length;
    return {
        count: characters + words,
        minutes: Math.max(1, Math.ceil(characters / 400 + words / 200)),
    };
}
