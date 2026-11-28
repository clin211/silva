export const siteName = 'Silva';
export const siteDescription = '记录工程实践、学习笔记和 AI 开源趋势。';

export const navigation = [
    { key: 'home', label: '首页', href: '/' },
    { key: 'blog', label: '博客', href: '/series/' },
    { key: 'trending', label: 'AI 趋势', href: '/ai-trending/' },
    { key: 'about', label: '关于', href: '/about/' },
] as const;

export type Section = (typeof navigation)[number]['key'];

export const postUrl = (slug: string) => `/blog/${slug}/`;
export const seriesUrl = (slug: string) => `/series/${slug}/`;
export const seriesDomainUrl = (slug: string) => `/series/?domain=${encodeURIComponent(slug)}`;
export const seriesCategoryUrl = seriesDomainUrl;
export const categoryUrl = (slug: string) => `/categories/${slug}/`;
export const tagUrl = (slug: string) => `/tags/${slug}/`;
export const repoUrl = (slug: string) => `/ai-trending/${slug}/`;
