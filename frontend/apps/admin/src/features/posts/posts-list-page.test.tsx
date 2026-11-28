import type { Schemas } from '@silva/api-client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useEffect, useState, type ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PostsListPage, type PostsSearchParams } from './posts-list-page';
import { ToastProvider } from '@/components/ui/toast';
import { SeriesListPage } from '@/features/series/series-list-page';
import { PERMISSIONS } from '@/lib/permissions';

let currentSearch: PostsSearchParams = {};
const searchListeners = new Set<() => void>();
const mockNavigate = vi.fn(
    (opts: { search?: PostsSearchParams | ((prev: PostsSearchParams) => PostsSearchParams) }) => {
        if (!opts || opts.search === undefined) return;
        const next = typeof opts.search === 'function' ? opts.search(currentSearch) : opts.search;
        currentSearch = next;
        for (const listener of searchListeners) {
            listener();
        }
    },
);
const mockGet = vi.fn();
const mockDelete = vi.fn();
let visiblePosts: Schemas['PostSummary'][];
let permissions: string[];

vi.mock('@tanstack/react-router', () => ({
    Link: ({
        children,
        to,
        search,
        params,
        ...props
    }: {
        children: ReactNode;
        to: string;
        search?: Record<string, unknown>;
        params?: Record<string, string>;
    }) => {
        let href = to;
        if (params) {
            for (const [k, v] of Object.entries(params)) {
                href = href.replace(`$${k}`, v);
            }
        }
        if (search && Object.keys(search).length > 0) {
            const qs = new URLSearchParams(
                Object.entries(search)
                    .filter(([, v]) => v !== undefined)
                    .map(([k, v]) => [k, String(v)]),
            ).toString();
            if (qs) href = `${href}?${qs}`;
        }
        return (
            <a href={href} {...props}>
                {children}
            </a>
        );
    },
    useNavigate: () => mockNavigate,
    useSearch: () => currentSearch,
}));

vi.mock('@/features/auth/auth-context', () => ({
    useAuth: () => ({
        user: {
            id: 'user-1',
            email: 'admin@example.com',
            display_name: 'Admin',
            roles: ['admin'],
            permissions,
        },
    }),
}));

vi.mock('@/lib/api', () => ({
    apiClient: {
        DELETE: (...args: unknown[]) => mockDelete(...args),
        GET: (...args: unknown[]) => mockGet(...args),
    },
    unwrapResponse: async <T,>(promise: Promise<{ data: T }>) => {
        const res = await promise;
        return res.data;
    },
}));

const MOCK_CATEGORY_COUNTS: Schemas['PostCategoryCounts'] = {
    total: 12,
    uncategorized: 2,
    categories: [
        {
            id: 'cat-backend',
            slug: 'backend',
            name: '后端架构',
            description: '服务端架构与工程实践',
            post_count: 6,
        },
        {
            id: 'cat-frontend',
            slug: 'frontend',
            name: '前端开发',
            description: 'React 与 TypeScript',
            post_count: 4,
        },
        {
            id: 'cat-devops',
            slug: 'devops',
            name: '运维部署',
            description: null,
            post_count: 1,
        },
    ],
};

const MOCK_POSTS: [Schemas['PostSummary'], Schemas['PostSummary']] = [
    {
        id: 'post-1',
        slug: 'rust-axum-guide',
        slug_locked: false,
        title: 'Rust Axum 实战指南',
        categories: [
            { id: 'cat-backend', slug: 'backend', name: '后端架构' },
            { id: 'cat-frontend', slug: 'frontend', name: '前端开发' },
            { id: 'cat-devops', slug: 'devops', name: '运维部署' },
            { id: 'cat-extra', slug: 'extra', name: '扩展专题' },
        ],
        author: { id: 'user-1', display_name: 'Admin' },
        current_revision_id: 'rev-1',
        current_revision_no: 2,
        published_revision_id: null,

        publication_version: 1,
        created_at: '2026-11-20T08:00:00Z',
        updated_at: '2026-11-25T10:30:00Z',
    },
    {
        id: 'post-2',
        slug: 'uncategorized-note',
        slug_locked: false,
        title: '随手记一篇未分类文章',
        categories: [],
        author: { id: 'user-1', display_name: 'Admin' },
        current_revision_id: 'rev-2',
        current_revision_no: 1,
        published_revision_id: null,

        publication_version: 1,
        created_at: '2026-11-21T08:00:00Z',
        updated_at: '2026-11-24T10:30:00Z',
    },
];

function Harness() {
    const [, setTick] = useState(0);
    useEffect(() => {
        const listener = () => setTick((n) => n + 1);
        searchListeners.add(listener);
        return () => {
            searchListeners.delete(listener);
        };
    }, []);
    return <PostsListPage />;
}

function renderPostsList(initialSearch: PostsSearchParams = {}, page = <Harness />) {
    currentSearch = initialSearch;
    const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    return render(
        <QueryClientProvider client={queryClient}>
            <ToastProvider>{page}</ToastProvider>
        </QueryClientProvider>,
    );
}

describe('PostsListPage', () => {
    beforeEach(() => {
        if (!Element.prototype.scrollIntoView) {
            Element.prototype.scrollIntoView = vi.fn();
        }
        if (typeof window.localStorage?.clear === 'function') {
            window.localStorage.clear();
        }
        currentSearch = {};
        mockNavigate.mockClear();
        mockGet.mockReset();
        mockDelete.mockReset();
        visiblePosts = [...MOCK_POSTS];
        permissions = [PERMISSIONS.POSTS_CREATE, PERMISSIONS.POSTS_MANAGE];
        mockGet.mockImplementation(
            (path: string, opts?: { params?: { query?: Record<string, unknown> } }) => {
                if (path === '/api/v1/admin/posts/category-counts') {
                    return Promise.resolve({ data: { data: MOCK_CATEGORY_COUNTS } });
                }
                if (path === '/api/v1/admin/categories') {
                    return Promise.resolve({
                        data: {
                            data: MOCK_CATEGORY_COUNTS.categories.map((c) => ({
                                ...c,
                                revision_count: c.post_count,
                                created_at: '2026-11-01T00:00:00Z',
                                updated_at: '2026-11-01T00:00:00Z',
                            })),
                        },
                    });
                }
                if (path === '/api/v1/admin/tags') {
                    return Promise.resolve({
                        data: {
                            data: [
                                {
                                    id: 'tag-rust',
                                    slug: 'rust',
                                    name: 'Rust',
                                    post_count: 3,
                                    revision_count: 3,
                                    created_at: '2026-11-01T00:00:00Z',
                                    updated_at: '2026-11-01T00:00:00Z',
                                },
                            ],
                        },
                    });
                }
                if (path === '/api/v1/admin/series') {
                    return Promise.resolve({
                        data: {
                            data: [],
                            meta: {
                                pagination: {
                                    page: 1,
                                    per_page: 20,
                                    total: 0,
                                    total_pages: 0,
                                },
                            },
                        },
                    });
                }
                if (path === '/api/v1/admin/posts') {
                    const perPage = Number(opts?.params?.query?.per_page ?? 20);
                    return Promise.resolve({
                        data: {
                            data: visiblePosts,
                            meta: {
                                pagination: {
                                    page: Number(opts?.params?.query?.page ?? 1),
                                    per_page: perPage,
                                    total: visiblePosts.length,
                                    total_pages: 1,
                                },
                            },
                        },
                    });
                }
                return Promise.resolve({ data: { data: [] } });
            },
        );
    });

    it('confirms the selected draft before deleting and refreshes the list', async () => {
        const user = userEvent.setup();
        const [post, other] = MOCK_POSTS;
        mockDelete.mockImplementation(async () => {
            visiblePosts = [other];
            return { data: undefined };
        });
        renderPostsList();
        const remove = await screen.findByRole('button', { name: `删除「${post.title}」` });
        await user.click(remove);
        const dialog = screen.getByRole('alertdialog', { name: `删除文章「${post.title}」？` });
        await user.click(within(dialog).getByRole('button', { name: '取消' }));
        expect(mockDelete).not.toHaveBeenCalled();
        await user.click(remove);
        await user.click(
            within(screen.getByRole('alertdialog')).getByRole('button', { name: '确认删除' }),
        );
        expect(mockDelete).toHaveBeenCalledWith('/api/v1/admin/posts/{id}', {
            params: {
                path: { id: post.id },
                query: {
                    expected_revision_id: post.current_revision_id,
                    expected_publication_version: post.publication_version,
                },
            },
        });
        await waitFor(() => expect(screen.queryByRole('link', { name: post.title })).toBeNull());
        expect(screen.getByRole('link', { name: other.title })).toBeInTheDocument();
    });

    it('deletes a standalone article from the series page and updates its count', async () => {
        permissions.push(PERMISSIONS.SERIES_MANAGE);
        const user = userEvent.setup();
        const [post, other] = MOCK_POSTS;
        mockDelete.mockImplementation(async () => {
            visiblePosts = [other];
            return { data: undefined };
        });
        renderPostsList({}, <SeriesListPage />);
        const section = within(await screen.findByRole('region', { name: '未归属专栏的独立文章' }));
        expect(section.getByText('2 篇')).toBeInTheDocument();
        await user.click(section.getByRole('button', { name: `删除「${post.title}」` }));
        const dialog = screen.getByRole('alertdialog', { name: `删除文章「${post.title}」？` });
        expect(mockDelete).not.toHaveBeenCalled();
        await user.click(within(dialog).getByRole('button', { name: '确认删除' }));
        expect(await section.findByText('1 篇')).toBeInTheDocument();
        expect(section.queryByRole('link', { name: post.title })).toBeNull();
        expect(section.getByRole('link', { name: other.title })).toBeInTheDocument();
        expect(mockNavigate).not.toHaveBeenCalled();
    });

    it('requires withdrawal before deleting a published post', async () => {
        const user = userEvent.setup();
        visiblePosts = [{ ...MOCK_POSTS[0], published_revision_id: 'live-rev' }];
        renderPostsList();
        await user.click(
            await screen.findByRole('button', { name: `删除「${MOCK_POSTS[0].title}」` }),
        );
        expect(await screen.findByText('请先到编辑页取消发布，再删除文章。')).toBeInTheDocument();
        expect(screen.queryByRole('alertdialog')).toBeNull();
        expect(mockDelete).not.toHaveBeenCalled();
    });

    it('hides delete from authors', async () => {
        permissions = [PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_EDIT_OWN];
        renderPostsList();
        await screen.findByRole('table');
        expect(screen.queryByRole('button', { name: /^删除/ })).toBeNull();
    });

    it('renders left category directory with total, uncategorized, and category counts', async () => {
        renderPostsList();

        const sidebar = screen.getByRole('complementary', { name: '分类目录' });
        await waitFor(() => {
            expect(
                within(sidebar).getByRole('button', { name: /全部文章.*12/ }),
            ).toBeInTheDocument();
        });
        expect(within(sidebar).getByRole('button', { name: /未分类.*2/ })).toBeInTheDocument();
        expect(within(sidebar).getByRole('button', { name: /后端架构.*6/ })).toBeInTheDocument();
        expect(within(sidebar).getByRole('button', { name: /前端开发.*4/ })).toBeInTheDocument();
        expect(within(sidebar).getByRole('button', { name: /运维部署.*1/ })).toBeInTheDocument();
    });

    it('filters by clicking a category or uncategorized in the directory and updates create link', async () => {
        const user = userEvent.setup();
        renderPostsList();

        const sidebar = screen.getByRole('complementary', { name: '分类目录' });
        const backendBtn = await within(sidebar).findByRole('button', { name: /后端架构/ });
        await user.click(backendBtn);

        await waitFor(() => {
            expect(mockGet).toHaveBeenCalledWith(
                '/api/v1/admin/posts',
                expect.objectContaining({
                    params: {
                        query: expect.objectContaining({
                            category_id: 'cat-backend',
                            uncategorized: undefined,
                            page: 1,
                        }),
                    },
                }),
            );
        });

        // 「新建文章」链接携带当前分类 ID
        const createLink = screen.getByRole('link', { name: /新建文章/ });
        expect(createLink).toHaveAttribute('href', '/posts/new?category_id=cat-backend');

        // 切换到「未分类」时清除 category_id 并传入 uncategorized=true
        const uncategorizedBtn = within(sidebar).getByRole('button', { name: /未分类/ });
        await user.click(uncategorizedBtn);

        await waitFor(() => {
            expect(mockGet).toHaveBeenCalledWith(
                '/api/v1/admin/posts',
                expect.objectContaining({
                    params: {
                        query: expect.objectContaining({
                            category_id: undefined,
                            uncategorized: true,
                            page: 1,
                        }),
                    },
                }),
            );
        });
    });

    it('renders category pills with overflow badge and uncategorized button on article rows, and filters on click', async () => {
        const user = userEvent.setup();
        renderPostsList();

        const table = await screen.findByRole('table');
        expect(within(table).getByText('Rust Axum 实战指南')).toBeInTheDocument();
        expect(within(table).getByRole('button', { name: '后端架构' })).toBeInTheDocument();
        expect(within(table).getByRole('button', { name: '前端开发' })).toBeInTheDocument();
        expect(within(table).getByRole('button', { name: '运维部署' })).toBeInTheDocument();
        // 第 4 个分类折叠为 +1
        const overflowBadge = within(table).getByText('+1');
        expect(overflowBadge).toHaveAttribute('title', '扩展专题');
        // 无分类文章展示「未分类」按钮
        expect(within(table).getByRole('button', { name: '未分类' })).toBeInTheDocument();

        await user.click(within(table).getByRole('button', { name: '前端开发' }));
        await waitFor(() => {
            expect(currentSearch).toMatchObject({
                category_id: 'cat-frontend',
                uncategorized: undefined,
                page: 1,
            });
        });

        const updatedTable = await screen.findByRole('table');
        await user.click(within(updatedTable).getByRole('button', { name: '未分类' }));
        await waitFor(() => {
            expect(currentSearch).toMatchObject({
                category_id: undefined,
                uncategorized: true,
                page: 1,
            });
        });
    });

    it('filters the category directory locally via sidebar search input', async () => {
        const user = userEvent.setup();
        renderPostsList();

        const sidebar = screen.getByRole('complementary', { name: '分类目录' });
        await within(sidebar).findByRole('button', { name: /后端架构/ });

        const searchInput = within(sidebar).getByLabelText('筛选分类');
        await user.type(searchInput, 'frontend');

        expect(within(sidebar).getByRole('button', { name: /前端开发/ })).toBeInTheDocument();
        expect(within(sidebar).queryByRole('button', { name: /后端架构/ })).not.toBeInTheDocument();

        // 清空筛选后恢复全部
        await user.click(within(sidebar).getByRole('button', { name: '清空分类筛选' }));
        expect(within(sidebar).getByRole('button', { name: /后端架构/ })).toBeInTheDocument();
    });

    it('switches sort order and per_page and re-queries posts', async () => {
        const user = userEvent.setup();
        renderPostsList();

        await screen.findByRole('table');

        // 切换排序方式为「标题 A→Z」
        const sortTrigger = screen.getByRole('button', { name: '排序方式' });
        await user.click(sortTrigger);
        const titleAscOption = await screen.findByRole('menuitemradio', { name: '标题 A→Z' });
        await user.click(titleAscOption);

        await waitFor(() => {
            expect(mockGet).toHaveBeenCalledWith(
                '/api/v1/admin/posts',
                expect.objectContaining({
                    params: {
                        query: expect.objectContaining({
                            sort: 'title_asc',
                            page: 1,
                        }),
                    },
                }),
            );
        });

        // 切换每页条数为 50
        const perPageSelect = screen.getByLabelText('每页显示条数');
        await user.selectOptions(perPageSelect, '50');

        await waitFor(() => {
            expect(mockGet).toHaveBeenCalledWith(
                '/api/v1/admin/posts',
                expect.objectContaining({
                    params: {
                        query: expect.objectContaining({
                            per_page: 50,
                            page: 1,
                        }),
                    },
                }),
            );
        });
    });
});
