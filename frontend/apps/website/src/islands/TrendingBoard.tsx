import { useRef, useSyncExternalStore } from 'react';
import type { Repository, WindowDays } from '../lib/trending';
import { growth, growthClass, number } from '../lib/format';
import { defaultFilters, pageSize, rank, sortOptions, type Filters } from '../lib/ranking';
import { repoUrl } from '../lib/site';

type OptionField = 'lang' | 'topic' | 'cat';
const optionFields = [
    ['lang', '语言', '全部语言'],
    ['topic', 'Topic', '全部 Topic'],
    ['cat', '分类', '全部分类'],
] as const;
const filtersChanged = 'silva:trending-filters-change';

function subscribeToSearch(sync: () => void) {
    window.addEventListener('popstate', sync);
    window.addEventListener(filtersChanged, sync);
    return () => {
        window.removeEventListener('popstate', sync);
        window.removeEventListener(filtersChanged, sync);
    };
}

const getSearch = () => window.location.search;
const getStaticSearch = () => null;

function readFilters(params: URLSearchParams): Filters {
    const sort =
        sortOptions.find(([key]) => key === params.get('sort'))?.[0] ?? defaultFilters.sort;
    const page = Number(params.get('page'));
    return {
        window: params.get('window') === '30' ? 30 : 7,
        sort,
        lang: params.get('lang') ?? '',
        topic: params.get('topic') ?? '',
        cat: params.get('cat') ?? '',
        page: Number.isSafeInteger(page) && page > 0 ? page : 1,
    };
}

function validOptions(current: string, options: string[]) {
    return current && !options.includes(current) ? [current, ...options] : options;
}

function Growth({ value }: { value: number | null }) {
    return <span className={growthClass(value)}>{growth(value)}</span>;
}

export default function TrendingBoard({ repos }: { repos: Repository[] }) {
    const search = useSyncExternalStore<string | null>(
        subscribeToSearch,
        getSearch,
        getStaticSearch,
    );
    const ready = search !== null;
    const filters = ready ? readFilters(new URLSearchParams(search)) : defaultFilters;
    const rankTitle = useRef<HTMLHeadingElement>(null);

    const options: Record<OptionField, string[]> = {
        lang: [...new Set(repos.map((repo) => repo.language))].sort(),
        topic: [...new Set(repos.flatMap((repo) => repo.topics))].sort(),
        cat: [...new Set(repos.map((repo) => repo.category))],
    };
    const featured = repos.filter((repo) => repo.featured);
    const ranked = rank(repos, filters);
    const pages = Math.max(1, Math.ceil(ranked.length / pageSize));
    const page = Math.min(pages, filters.page);
    const start = (page - 1) * pageSize;
    const results = ranked.slice(start, start + pageSize);
    const windowDays = filters.window;

    function navigate(next: Filters) {
        const search = new URLSearchParams(location.search);
        for (const key of Object.keys(defaultFilters) as Array<keyof Filters>) {
            if (defaultFilters[key] === next[key]) search.delete(key);
            else search.set(key, String(next[key]));
        }

        const url = new URL(location.href);
        url.search = search.toString();
        if (url.href === location.href) return;
        history.pushState(null, '', url);
        window.dispatchEvent(new Event(filtersChanged));
    }

    function change(patch: Partial<Filters>) {
        navigate({ ...filters, ...patch, page: 1 });
    }

    function goToPage(nextPage: number) {
        navigate({ ...filters, page: nextPage });
        rankTitle.current?.focus({ preventScroll: true });
        rankTitle.current?.scrollIntoView({ block: 'start' });
    }

    return (
        <>
            <section aria-label="榜单筛选" className="trending-controls">
                <noscript>
                    <p className="t-small muted">启用 JavaScript 后可使用筛选和分页。</p>
                </noscript>
                <fieldset disabled={!ready} hidden={!ready}>
                    <legend className="sr-only">榜单筛选</legend>
                    <div className="filter-window">
                        <span className="filter-label" id="window-label">
                            时间范围
                        </span>
                        <fieldset className="choice" aria-labelledby="window-label">
                            {([7, 30] as WindowDays[]).map((days) => (
                                <button
                                    key={days}
                                    type="button"
                                    aria-pressed={days === windowDays}
                                    onClick={() => change({ window: days })}
                                >
                                    近 {days} 天
                                </button>
                            ))}
                        </fieldset>
                    </div>
                    <div>
                        <label className="filter-label" htmlFor="filter-sort">
                            排序
                        </label>
                        <select
                            className="select"
                            id="filter-sort"
                            value={filters.sort}
                            onChange={(event) =>
                                change({ sort: event.target.value as Filters['sort'] })
                            }
                        >
                            {sortOptions.map(([key, label]) => (
                                <option key={key} value={key}>
                                    {label}
                                </option>
                            ))}
                        </select>
                    </div>
                    {optionFields.map(([field, label, all]) => (
                        <div key={field}>
                            <label className="filter-label" htmlFor={`filter-${field}`}>
                                {label}
                            </label>
                            <select
                                className="select"
                                id={`filter-${field}`}
                                value={filters[field]}
                                onChange={(event) => change({ [field]: event.target.value })}
                            >
                                <option value="">{all}</option>
                                {validOptions(filters[field], options[field]).map((value) => (
                                    <option key={value}>{value}</option>
                                ))}
                            </select>
                        </div>
                    ))}
                </fieldset>
            </section>

            {featured.length > 0 && (
                <section className="trend-section" aria-labelledby="featured-title">
                    <h2 id="featured-title" className="t-h2">
                        编辑精选
                    </h2>
                    <ul className="featured-repos">
                        {featured.map((repo) => (
                            <li className="bezel lift" key={repo.slug}>
                                <a className="face repo-card" href={repoUrl(repo.slug)}>
                                    <p className="metadata">
                                        <span className="chip chip-teal">{repo.category}</span>
                                        <span>{repo.language}</span>
                                    </p>
                                    <h3 className="t-h3 hover-accent">{repo.name}</h3>
                                    <p className="ink-2">{repo.description}</p>
                                    <p className="repo-card-metric t-small">
                                        <span className="muted">近 {windowDays} 天 Stars</span>
                                        <span className="num">
                                            <Growth value={repo.growth.stars[windowDays]} />
                                        </span>
                                    </p>
                                </a>
                            </li>
                        ))}
                    </ul>
                </section>
            )}

            <section className="trend-section" aria-labelledby="rank-title">
                <header className="section-heading ranking-heading">
                    <h2 id="rank-title" className="t-h2" tabIndex={-1} ref={rankTitle}>
                        自然榜单
                    </h2>
                    <p className="t-small muted num" aria-live="polite">
                        {ranked.length
                            ? `共 ${ranked.length} 个仓库 · 第 ${page} / ${pages} 页`
                            : '没有符合条件的结果'}
                    </p>
                </header>
                {results.length ? (
                    <>
                        <div className="panel rank-table">
                            {/* oxlint-disable jsx-a11y/no-redundant-roles, jsx-a11y/no-interactive-element-to-noninteractive-role -- Mobile CSS uses grid cards. Explicit roles preserve table semantics. */}
                            <table className="data-table" role="table" aria-labelledby="rank-title">
                                <thead role="rowgroup">
                                    <tr role="row">
                                        <th role="columnheader" scope="col">
                                            排名
                                        </th>
                                        <th role="columnheader" scope="col">
                                            仓库
                                        </th>
                                        <th role="columnheader" scope="col">
                                            语言
                                        </th>
                                        <th role="columnheader" scope="col" className="num">
                                            近 {windowDays} 天 Stars
                                        </th>
                                        <th role="columnheader" scope="col" className="num">
                                            近 {windowDays} 天 Forks
                                        </th>
                                        <th role="columnheader" scope="col" className="num">
                                            Stars 总数
                                        </th>
                                        <th role="columnheader" scope="col" className="num">
                                            最近更新
                                        </th>
                                    </tr>
                                </thead>
                                <tbody role="rowgroup">
                                    {results.map((repo, index) => (
                                        <tr role="row" key={repo.slug}>
                                            <td role="cell" className="row-rank">
                                                <span
                                                    className={
                                                        start + index < 3
                                                            ? 'rank-pill podium'
                                                            : 'rank-pill'
                                                    }
                                                >
                                                    {start + index + 1}
                                                </span>
                                            </td>
                                            <th role="rowheader" scope="row" className="row-repo">
                                                <a
                                                    className="hover-accent"
                                                    href={repoUrl(repo.slug)}
                                                >
                                                    {repo.name}
                                                </a>
                                                <p className="t-small muted">{repo.description}</p>
                                            </th>
                                            <td role="cell" className="row-language">
                                                <span className="chip">{repo.language}</span>
                                            </td>
                                            <td role="cell" className="num row-metric">
                                                <span
                                                    className="mobile-metric muted"
                                                    aria-hidden="true"
                                                >
                                                    近 {windowDays} 天 Stars
                                                </span>
                                                <Growth value={repo.growth.stars[windowDays]} />
                                            </td>
                                            <td role="cell" className="num row-metric">
                                                <span
                                                    className="mobile-metric muted"
                                                    aria-hidden="true"
                                                >
                                                    近 {windowDays} 天 Forks
                                                </span>
                                                <Growth value={repo.growth.forks[windowDays]} />
                                            </td>
                                            <td role="cell" className="num row-metric">
                                                <span
                                                    className="mobile-metric muted"
                                                    aria-hidden="true"
                                                >
                                                    Stars 总数
                                                </span>
                                                {number(repo.stars)}
                                            </td>
                                            <td role="cell" className="num row-updated">
                                                <time dateTime={repo.updated}>
                                                    {repo.updated.slice(5)}
                                                </time>
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                            {/* oxlint-enable jsx-a11y/no-redundant-roles, jsx-a11y/no-interactive-element-to-noninteractive-role */}
                        </div>
                        {ready && pages > 1 && (
                            <nav className="pager" aria-label="榜单分页">
                                <button
                                    className="btn btn-ghost btn-sm"
                                    type="button"
                                    disabled={page === 1}
                                    onClick={() => goToPage(page - 1)}
                                >
                                    上一页
                                </button>
                                {Array.from({ length: pages }, (_, index) => index + 1).map((n) => (
                                    <button
                                        className={`btn btn-sm ${page === n ? 'btn-dark' : 'btn-ghost'}`}
                                        aria-current={page === n ? 'page' : undefined}
                                        aria-label={`第 ${n} 页`}
                                        type="button"
                                        key={n}
                                        onClick={() => goToPage(n)}
                                    >
                                        {n}
                                    </button>
                                ))}
                                <button
                                    className="btn btn-ghost btn-sm"
                                    type="button"
                                    disabled={page === pages}
                                    onClick={() => goToPage(page + 1)}
                                >
                                    下一页
                                </button>
                            </nav>
                        )}
                    </>
                ) : (
                    <div className="panel empty-state">
                        <h3 className="t-h3">没有符合条件的结果</h3>
                        <p className="ink-2">可以更换语言、Topic 或分类。</p>
                        <button
                            type="button"
                            className="btn btn-dark btn-sm"
                            onClick={() => navigate(defaultFilters)}
                        >
                            清除筛选
                        </button>
                    </div>
                )}
            </section>
        </>
    );
}
