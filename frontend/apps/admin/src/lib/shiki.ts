import {
    createHighlighterCore,
    getTokenStyleObject,
    stringifyTokenStyle,
    type HighlighterCore,
    type LanguageInput,
    type ThemedToken,
} from '@shikijs/core';
import { createJavaScriptRegexEngine } from '@shikijs/engine-javascript';

/** 与公开站点 Astro Markdown 默认主题保持一致。 */
export const SHIKI_THEME = 'github-dark' as const;

export interface CodeLanguageOption {
    /** 写入 Markdown fenced code block 的语言标识，空字符串表示纯文本。 */
    value: string;
    /** 界面显示名称。 */
    label: string;
}

const LANGUAGE_LOADERS = {
    typescript: () => import('@shikijs/langs/typescript'),
    javascript: () => import('@shikijs/langs/javascript'),
    tsx: () => import('@shikijs/langs/tsx'),
    jsx: () => import('@shikijs/langs/jsx'),
    rust: () => import('@shikijs/langs/rust'),
    go: () => import('@shikijs/langs/go'),
    python: () => import('@shikijs/langs/python'),
    java: () => import('@shikijs/langs/java'),
    c: () => import('@shikijs/langs/c'),
    cpp: () => import('@shikijs/langs/cpp'),
    csharp: () => import('@shikijs/langs/csharp'),
    bash: () => import('@shikijs/langs/bash'),
    sql: () => import('@shikijs/langs/sql'),
    json: () => import('@shikijs/langs/json'),
    jsonc: () => import('@shikijs/langs/jsonc'),
    yaml: () => import('@shikijs/langs/yaml'),
    toml: () => import('@shikijs/langs/toml'),
    html: () => import('@shikijs/langs/html'),
    css: () => import('@shikijs/langs/css'),
    scss: () => import('@shikijs/langs/scss'),
    markdown: () => import('@shikijs/langs/markdown'),
    dockerfile: () => import('@shikijs/langs/dockerfile'),
    diff: () => import('@shikijs/langs/diff'),
    nginx: () => import('@shikijs/langs/nginx'),
    graphql: () => import('@shikijs/langs/graphql'),
    xml: () => import('@shikijs/langs/xml'),
    ini: () => import('@shikijs/langs/ini'),
    astro: () => import('@shikijs/langs/astro'),
    vue: () => import('@shikijs/langs/vue'),
    svelte: () => import('@shikijs/langs/svelte'),
} satisfies Record<string, LanguageInput>;

export type SupportedShikiLanguage = keyof typeof LANGUAGE_LOADERS;

export const CODE_LANGUAGE_OPTIONS: readonly CodeLanguageOption[] = [
    { value: '', label: '纯文本' },
    { value: 'typescript', label: 'TypeScript' },
    { value: 'javascript', label: 'JavaScript' },
    { value: 'tsx', label: 'TSX' },
    { value: 'jsx', label: 'JSX' },
    { value: 'rust', label: 'Rust' },
    { value: 'go', label: 'Go' },
    { value: 'python', label: 'Python' },
    { value: 'java', label: 'Java' },
    { value: 'c', label: 'C' },
    { value: 'cpp', label: 'C++' },
    { value: 'csharp', label: 'C#' },
    { value: 'bash', label: 'Bash / Shell' },
    { value: 'sql', label: 'SQL' },
    { value: 'json', label: 'JSON' },
    { value: 'jsonc', label: 'JSONC' },
    { value: 'yaml', label: 'YAML' },
    { value: 'toml', label: 'TOML' },
    { value: 'html', label: 'HTML' },
    { value: 'css', label: 'CSS' },
    { value: 'scss', label: 'SCSS' },
    { value: 'markdown', label: 'Markdown' },
    { value: 'dockerfile', label: 'Dockerfile' },
    { value: 'diff', label: 'Diff' },
    { value: 'nginx', label: 'Nginx' },
    { value: 'graphql', label: 'GraphQL' },
    { value: 'xml', label: 'XML' },
    { value: 'ini', label: 'INI' },
    { value: 'astro', label: 'Astro' },
    { value: 'vue', label: 'Vue' },
    { value: 'svelte', label: 'Svelte' },
];

const LANGUAGE_LABELS = new Map<string, string>(
    CODE_LANGUAGE_OPTIONS.map((item) => [item.value, item.label]),
);

const LANGUAGE_ALIASES: Record<string, SupportedShikiLanguage> = {
    ts: 'typescript',
    mts: 'typescript',
    cts: 'typescript',
    js: 'javascript',
    mjs: 'javascript',
    cjs: 'javascript',
    rs: 'rust',
    golang: 'go',
    py: 'python',
    'c++': 'cpp',
    cc: 'cpp',
    cxx: 'cpp',
    hpp: 'cpp',
    'c#': 'csharp',
    cs: 'csharp',
    sh: 'bash',
    shell: 'bash',
    shellscript: 'bash',
    zsh: 'bash',
    yml: 'yaml',
    md: 'markdown',
    docker: 'dockerfile',
    gql: 'graphql',
    conf: 'ini',
    properties: 'ini',
};

const PLAIN_TEXT_ALIASES = new Set(['', 'text', 'txt', 'plain', 'plaintext']);

/**
 * 将任意 Markdown fenced code language 归一化为当前按需支持的 Shiki 语言 ID；
 * 纯文本或未收录语言返回 null。
 */
export function resolveShikiLanguage(
    raw: string | null | undefined,
): SupportedShikiLanguage | null {
    if (!raw) return null;
    const normalized = raw.trim().toLowerCase();
    if (PLAIN_TEXT_ALIASES.has(normalized)) return null;
    if (normalized in LANGUAGE_LOADERS) {
        return normalized as SupportedShikiLanguage;
    }
    return LANGUAGE_ALIASES[normalized] ?? null;
}

/**
 * 返回适合界面展示的语言标签。未知自定义语言原样显示。
 */
export function getCodeLanguageLabel(raw: string | null | undefined): string {
    const trimmed = raw?.trim() ?? '';
    if (!trimmed) return '纯文本';
    const resolved = resolveShikiLanguage(trimmed);
    if (resolved) {
        return LANGUAGE_LABELS.get(resolved) ?? resolved;
    }
    const lower = trimmed.toLowerCase();
    if (PLAIN_TEXT_ALIASES.has(lower)) return '纯文本';
    return trimmed;
}

let highlighterInstance: HighlighterCore | null = null;
let highlighterPromise: Promise<HighlighterCore> | null = null;
const loadingLanguages = new Map<SupportedShikiLanguage, Promise<boolean>>();

async function getHighlighter(): Promise<HighlighterCore> {
    if (highlighterInstance) return highlighterInstance;
    if (!highlighterPromise) {
        highlighterPromise = createHighlighterCore({
            themes: [import('@shikijs/themes/github-dark')],
            langs: [],
            engine: createJavaScriptRegexEngine({ forgiving: true }),
        }).then((instance) => {
            highlighterInstance = instance;
            return instance;
        });
    }
    return highlighterPromise;
}

/**
 * 同步判断指定语言是否已在 Shiki 单例中加载完毕，供 ProseMirror Decoration 同步渲染使用。
 */
export function isShikiLanguageLoaded(lang: string | null | undefined): boolean {
    const resolved = resolveShikiLanguage(lang);
    if (!resolved || !highlighterInstance) return false;
    return highlighterInstance.getLoadedLanguages().includes(resolved);
}

/**
 * 按需异步加载单个语言的 TextMate 语法并缓存，重复调用自动去重。
 */
export async function ensureShikiLanguage(lang: string | null | undefined): Promise<boolean> {
    const resolved = resolveShikiLanguage(lang);
    if (!resolved) return false;
    if (isShikiLanguageLoaded(resolved)) return true;

    const existing = loadingLanguages.get(resolved);
    if (existing) return existing;

    const task = (async () => {
        try {
            const highlighter = await getHighlighter();
            if (!highlighter.getLoadedLanguages().includes(resolved)) {
                await highlighter.loadLanguage(LANGUAGE_LOADERS[resolved]);
            }
            return highlighter.getLoadedLanguages().includes(resolved);
        } catch {
            return false;
        } finally {
            loadingLanguages.delete(resolved);
        }
    })();

    loadingLanguages.set(resolved, task);
    return task;
}

/**
 * 同步使用已加载的 Shiki 语法对代码分词；若语言尚未加载或不支持则返回 null。
 */
export function tokenizeCodeSync(
    code: string,
    lang: string | null | undefined,
): ThemedToken[][] | null {
    const resolved = resolveShikiLanguage(lang);
    if (!resolved || !highlighterInstance) return null;
    if (!highlighterInstance.getLoadedLanguages().includes(resolved)) return null;
    try {
        return highlighterInstance.codeToTokensBase(code, {
            lang: resolved,
            theme: SHIKI_THEME,
        });
    } catch {
        return null;
    }
}

/**
 * 异步按需加载语言后对代码分词，供预览区安全构建 DOM 高亮节点。
 */
export async function tokenizeCodeAsync(
    code: string,
    lang: string | null | undefined,
): Promise<ThemedToken[][] | null> {
    const ready = await ensureShikiLanguage(lang);
    if (!ready) return null;
    return tokenizeCodeSync(code, lang);
}

/**
 * 将 Shiki ThemedToken 转为安全的行内 CSS 字符串（仅含颜色与字形样式）。
 */
export function tokenToInlineStyle(token: ThemedToken): string {
    const styleObj = getTokenStyleObject(token);
    return stringifyTokenStyle(styleObj);
}

/**
 * 将 Shiki token 应用到 DOM span 元素的样式上（避免拼接 innerHTML）。
 */
export function applyTokenStyleToElement(element: HTMLElement, token: ThemedToken): void {
    const styleObj = getTokenStyleObject(token);
    if (styleObj.color) {
        element.style.color = styleObj.color;
    }
    if (styleObj['font-style']) {
        element.style.fontStyle = styleObj['font-style'];
    }
    if (styleObj['font-weight']) {
        element.style.fontWeight = styleObj['font-weight'];
    }
    if (styleObj['text-decoration']) {
        element.style.textDecoration = styleObj['text-decoration'];
    }
}
