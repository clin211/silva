export type Metric = 'stars' | 'forks';
export type WindowDays = 7 | 30;

export interface Sample {
    date: string;
    stars: number;
    forks: number;
}

export interface Repository {
    slug: string;
    name: string;
    description: string;
    language: string;
    topics: string[];
    category: string;
    stars: number;
    forks: number;
    updated: string;
    featured: boolean;
    growth: Record<Metric, Record<WindowDays, number | null>>;
}

export interface TrendingSnapshot {
    date: string;
    repos: Repository[];
}
