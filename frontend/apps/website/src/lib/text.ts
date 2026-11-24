const wordSegmenter = new Intl.Segmenter('zh-CN', { granularity: 'word' });

const entities: Record<string, string> = {
    amp: '&',
    lt: '<',
    gt: '>',
    quot: '"',
    apos: "'",
    nbsp: ' ',
};

export function decodeEntities(value: string) {
    return value.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (match, name: string) => {
        if (name[0] !== '#') return entities[name.toLowerCase()] ?? match;
        const code =
            name[1] === 'x' || name[1] === 'X'
                ? parseInt(name.slice(2), 16)
                : Number(name.slice(1));
        return Number.isFinite(code) && code <= 0x10ffff ? String.fromCodePoint(code) : match;
    });
}

/** Markdown 正文去掉代码块、图片和链接地址后的文本。 */
export function markdownText(markdown = '') {
    return markdown
        .replace(/^\s*(?:`{3,}|~{3,})[^\n]*\n[\s\S]*?^\s*(?:`{3,}|~{3,})\s*$/gm, '')
        .replace(/!\[([^\]]*)\]\([^)]+\)/g, '')
        .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1');
}

/** 消毒后的 HTML 去掉代码块和标签后的文本。 */
export function htmlText(html: string) {
    return decodeEntities(
        html
            .replace(/<pre[\s>][\s\S]*?<\/pre>/gi, ' ')
            .replace(/<[^>]*>/g, ' ')
            .replace(/\s+/g, ' ')
            .trim(),
    );
}

/** 只取段落，用于从正文生成摘要。 */
export const paragraphs = (html: string) => html.match(/<p[\s>][\s\S]*?<\/p>/g)?.join(' ') ?? html;

export function readingStats(text: string) {
    const characters = text.match(/\p{Script=Han}/gu)?.length ?? 0;
    const words = Array.from(wordSegmenter.segment(text)).filter(
        ({ segment, isWordLike }) => isWordLike && !/\p{Script=Han}/u.test(segment),
    ).length;
    return {
        count: characters + words,
        minutes: Math.max(1, Math.ceil(characters / 400 + words / 200)),
    };
}

export function summarize(text: string, limit = 120) {
    const chars = Array.from(text.trim());
    return chars.length <= limit ? chars.join('') : `${chars.slice(0, limit).join('').trimEnd()}…`;
}
