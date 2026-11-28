import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRouter, RouterProvider } from '@tanstack/react-router';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { routeTree } from '@/app/router';
import { ToastProvider } from '@/components/ui/toast';
import { PERMISSIONS, type RoleKey } from '@/lib/permissions';

let mockUser: {
    id: string;
    email: string;
    display_name: string;
    roles: RoleKey[];
    permissions: string[];
};

const mockGet = vi.fn();

vi.mock('@/features/auth/auth-context', () => ({
    useAuth: () => ({
        status: 'authenticated',
        user: mockUser,
        sessionExpired: false,
        logout: vi.fn(),
    }),
}));

vi.mock('@/lib/api', () => ({
    apiClient: {
        GET: (...args: unknown[]) => mockGet(...args),
    },
    fetchHealthLive: vi.fn().mockResolvedValue({ latencyMs: 12 }),
    unwrapResponse: async <T,>(promise: Promise<{ data: T }>) => {
        const res = await promise;
        return res.data;
    },
}));

function renderApp(initialPath: string) {
    const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    const testRouter = createRouter({
        routeTree,
        history: createMemoryHistory({ initialEntries: [initialPath] }),
    });
    render(
        <QueryClientProvider client={queryClient}>
            <ToastProvider>
                <RouterProvider router={testRouter} />
            </ToastProvider>
        </QueryClientProvider>,
    );
    return testRouter;
}

describe('AdminShell 博客管理菜单与导航', () => {
    beforeEach(() => {
        window.localStorage.clear();
        mockGet.mockReset();
        mockUser = {
            id: 'user-admin',
            email: 'admin@example.com',
            display_name: 'Admin',
            roles: ['admin'],
            permissions: [
                PERMISSIONS.POSTS_CREATE,
                PERMISSIONS.POSTS_READ_OWN,
                PERMISSIONS.POSTS_MANAGE,
                PERMISSIONS.SERIES_MANAGE,
                PERMISSIONS.TAXONOMY_MANAGE,
                PERMISSIONS.MEDIA_MANAGE,
            ],
        };
        mockGet.mockImplementation((path: string) => {
            if (path === '/api/v1/public/site') {
                return Promise.resolve({ data: { data: null } });
            }
            if (path === '/api/v1/admin/categories') {
                return Promise.resolve({
                    data: {
                        data: [
                            {
                                id: 'cat-1',
                                slug: 'rust',
                                name: 'Rust',
                                description: 'Rust 语言',
                                post_count: 3,
                                revision_count: 3,
                                created_at: '2026-11-01T00:00:00Z',
                                updated_at: '2026-11-01T00:00:00Z',
                            },
                        ],
                    },
                });
            }
            if (path === '/api/v1/admin/tags') {
                return Promise.resolve({
                    data: {
                        data: [
                            {
                                id: 'tag-1',
                                slug: 'tokio',
                                name: 'Tokio',
                                post_count: 2,
                                revision_count: 2,
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
                            pagination: { page: 1, per_page: 20, total: 0, total_pages: 0 },
                        },
                    },
                });
            }
            if (path === '/api/v1/admin/posts') {
                return Promise.resolve({
                    data: {
                        data: [],
                        meta: {
                            pagination: { page: 1, per_page: 20, total: 0, total_pages: 0 },
                        },
                    },
                });
            }
            if (path === '/api/v1/admin/posts/category-counts') {
                return Promise.resolve({
                    data: { data: { total: 0, uncategorized: 0, categories: [] } },
                });
            }
            return Promise.resolve({ data: { data: [] } });
        });
    });

    it('groups 专栏, 分类, and 标签 under 博客管理 without a standalone 分类与标签 menu item', async () => {
        const user = userEvent.setup();
        renderApp('/series');

        const nav = await screen.findByRole('navigation', { name: '后台导航' });
        expect(within(nav).getByRole('button', { name: '博客管理' })).toHaveAttribute(
            'aria-expanded',
            'true',
        );
        expect(within(nav).queryByRole('link', { name: '分类与标签' })).toBeNull();

        const submenu = within(nav).getByRole('list', { name: '博客管理子菜单' });
        const seriesLink = within(submenu).getByRole('link', { name: '专栏' });
        const categoriesLink = within(submenu).getByRole('link', { name: '分类' });
        const tagsLink = within(submenu).getByRole('link', { name: '标签' });

        expect(seriesLink).toHaveAttribute('aria-current', 'page');
        expect(categoriesLink).not.toHaveAttribute('aria-current');
        expect(tagsLink).not.toHaveAttribute('aria-current');

        const breadcrumb = screen.getByRole('navigation', { name: '面包屑' });
        expect(within(breadcrumb).getByText('博客管理')).toBeInTheDocument();
        expect(within(breadcrumb).getByText('专栏')).toBeInTheDocument();

        // 点击「分类」跳转到分类管理并同步侧栏激活态与面包屑
        await user.click(categoriesLink);
        await waitFor(() => {
            expect(screen.getByRole('heading', { name: '分类管理' })).toBeInTheDocument();
        });
        expect(within(submenu).getByRole('link', { name: '分类' })).toHaveAttribute(
            'aria-current',
            'page',
        );
        expect(within(breadcrumb).getByText('分类')).toBeInTheDocument();

        // 点击「标签」跳转到标签管理并同步侧栏激活态与面包屑
        await user.click(within(submenu).getByRole('link', { name: '标签' }));
        await waitFor(() => {
            expect(screen.getByRole('heading', { name: '标签管理' })).toBeInTheDocument();
        });
        expect(within(submenu).getByRole('link', { name: '标签' })).toHaveAttribute(
            'aria-current',
            'page',
        );
        expect(within(breadcrumb).getByText('标签')).toBeInTheDocument();
        expect(await screen.findByText('Tokio')).toBeInTheDocument();
    });

    it('toggles the 博客管理 submenu and supports compact sidebar flyout', async () => {
        const user = userEvent.setup();
        renderApp('/series');

        const nav = await screen.findByRole('navigation', { name: '后台导航' });
        const blogBtn = within(nav).getByRole('button', { name: '博客管理' });

        // 收起子菜单
        await user.click(blogBtn);
        expect(blogBtn).toHaveAttribute('aria-expanded', 'false');
        expect(within(nav).queryByRole('list', { name: '博客管理子菜单' })).toBeNull();

        // 收起整个侧栏进入紧凑模式并打开浮层菜单
        await user.click(screen.getByRole('button', { name: '收起侧栏' }));
        const compactBlogBtn = within(nav).getByRole('button', { name: '博客管理' });
        await user.click(compactBlogBtn);

        const flyout = screen.getByRole('menu', { name: '博客管理' });
        expect(within(flyout).getByRole('link', { name: '专栏' })).toBeInTheDocument();
        expect(within(flyout).getByRole('link', { name: '分类' })).toBeInTheDocument();
        expect(within(flyout).getByRole('link', { name: '标签' })).toBeInTheDocument();

        await user.click(within(flyout).getByRole('link', { name: '标签' }));
        await waitFor(() => {
            expect(screen.getByRole('heading', { name: '标签管理' })).toBeInTheDocument();
        });
        expect(screen.queryByRole('menu', { name: '博客管理' })).toBeNull();
    });

    it('renders a direct 博客管理 link for authors without series or taxonomy permissions', async () => {
        mockUser = {
            id: 'user-author',
            email: 'author@example.com',
            display_name: 'Author',
            roles: ['author'],
            permissions: [PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_CREATE],
        };
        renderApp('/posts');

        const nav = await screen.findByRole('navigation', { name: '后台导航' });
        const blogLink = within(nav).getByRole('link', { name: '博客管理' });
        expect(blogLink).toHaveAttribute('href', '/posts');
        expect(blogLink).toHaveAttribute('aria-current', 'page');
        expect(within(nav).queryByRole('link', { name: '分类' })).toBeNull();
        expect(within(nav).queryByRole('link', { name: '标签' })).toBeNull();
    });
});
