import type { Repository, WindowDays } from '../data/trending';

export const sortOptions = [
    ['stars_growth', 'Stars 净增长'],
    ['forks_growth', 'Forks 净增长'],
    ['stars', 'Stars 总数'],
    ['updated', '最近更新'],
] as const;

export interface Filters {
    window: WindowDays;
    sort: (typeof sortOptions)[number][0];
    lang: string;
    topic: string;
    cat: string;
    page: number;
}

export const defaultFilters: Filters = {
    window: 7,
    sort: 'stars_growth',
    lang: '',
    topic: '',
    cat: '',
    page: 1,
};
export const pageSize = 10;

export function rank(repos: Repository[], filters: Filters) {
    const selected = repos.filter(
        (repo) =>
            (!filters.lang || repo.language === filters.lang) &&
            (!filters.topic || repo.topics.includes(filters.topic)) &&
            (!filters.cat || repo.category === filters.cat),
    );

    return selected.sort((a, b) => {
        const byName = () => a.name.localeCompare(b.name, 'en');
        if (filters.sort === 'updated') return b.updated.localeCompare(a.updated) || byName();
        if (filters.sort === 'stars') return b.stars - a.stars || byName();

        const metric = filters.sort === 'forks_growth' ? 'forks' : 'stars';
        const left = a.growth[metric][filters.window] ?? -Infinity;
        const right = b.growth[metric][filters.window] ?? -Infinity;
        return (left === right ? b.stars - a.stars : right - left) || byName();
    });
}
