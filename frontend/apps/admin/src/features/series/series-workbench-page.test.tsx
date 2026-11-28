import type { Schemas } from '@silva/api-client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SeriesWorkbenchPage } from './series-workbench-page';
import { ToastProvider } from '@/components/ui/toast';
import { ApiError } from '@/lib/errors';
import { PERMISSIONS } from '@/lib/permissions';

const mockNavigate = vi.fn();
const mockGet = vi.fn();
const mockPost = vi.fn();
const mockPatch = vi.fn();
const mockPut = vi.fn();
const mockDelete = vi.fn();

vi.mock('@tanstack/react-router', () => ({
    Link: ({
        children,
        to,
        params,
        ...props
    }: {
        children: ReactNode;
        to: string;
        params?: Record<string, string>;
    }) => {
        let href = to;
        if (params) {
            for (const [k, v] of Object.entries(params)) {
                href = href.replace(`$${k}`, v);
            }
        }
        return (
            <a href={href} {...props}>
                {children}
            </a>
        );
    },
    useNavigate: () => mockNavigate,
}));

vi.mock('@/features/auth/auth-context', () => ({
    useAuth: () => ({
        user: {
            id: '019535d9-1a2b-7c3d-8e4f-5a6b7c8d9e0f',
            email: 'editor@example.com',
            display_name: 'Editor',
            roles: ['editor'],
            permissions: [
                PERMISSIONS.POSTS_CREATE,
                PERMISSIONS.POSTS_MANAGE,
                PERMISSIONS.POSTS_PUBLISH,
                PERMISSIONS.SERIES_MANAGE,
            ],
        },
    }),
}));

vi.mock('@/lib/api', () => ({
    apiClient: {
        GET: (...args: unknown[]) => mockGet(...args),
        POST: (...args: unknown[]) => mockPost(...args),
        PATCH: (...args: unknown[]) => mockPatch(...args),
        PUT: (...args: unknown[]) => mockPut(...args),
        DELETE: (...args: unknown[]) => mockDelete(...args),
    },
    uploadMediaFile: vi.fn(),
    fetchMediaBlob: vi.fn(),
    unwrapResponse: async <T,>(promise: Promise<{ data?: T; error?: unknown }>) => {
        const res = await promise;
        if (res.error) throw res.error;
        return res.data as T;
    },
}));

const SERIES_ID = '019535e0-0000-7000-8000-000000000001';
const SECTION_ID = '019535e1-0000-7000-8000-000000000001';
const POST_ID_1 = '019535d9-0000-7000-8000-000000000001';
const POST_ID_NEW = '019535d9-0000-7000-8000-000000000099';

function makeSeries(overrides: Partial<Schemas['Series']> = {}): Schemas['Series'] {
    return {
        id: SERIES_ID,
        slug: 'rust-concurrency',
        title: 'Rust 并发编程',
        description: '从线程到异步运行时',
        cover_media_id: null,
        category_id: '019535d8-7e21-7f00-a111-2233445566aa',
        completion_status: 'ongoing',
        is_featured: false,
        sort_order: 10,
        seo_title: null,
        seo_description: null,
        slug_locked: false,
        edit_version: 3,
        publication_version: 1,
        item_count: 1,
        published_post_count: 1,
        is_live: false,

        published_at: null,
        first_published_at: null,
        created_by: '019535d9-1a2b-7c3d-8e4f-5a6b7c8d9e0f',
        created_at: '2026-11-26T10:00:00+08:00',
        updated_at: '2026-11-26T11:00:00+08:00',
        items: [],
        sections: [
            {
                id: SECTION_ID,
                title: '一、基础并发原语',
                description: '线程与共享状态',
                position: 0,
                items: [
                    {
                        id: '019535e2-0000-7000-8000-000000000001',
                        post_id: POST_ID_1,
                        display_title: '线程基础',
                        section_id: SECTION_ID,
                        position: 0,
                        title: 'Rust 线程与作用域线程',
                        slug: 'rust-threads',
                        current_revision_id: '019535da-0000-7000-8000-000000000001',
                        published_revision_id: '019535da-0000-7000-8000-000000000001',
                    },
                ],
            },
        ],
        ...overrides,
    };
}

function renderWorkbench() {
    const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    return render(
        <QueryClientProvider client={queryClient}>
            <ToastProvider>
                <SeriesWorkbenchPage seriesId={SERIES_ID} />
            </ToastProvider>
        </QueryClientProvider>,
    );
}

describe('SeriesWorkbenchPage', () => {
    beforeEach(() => {
        if (!Element.prototype.scrollIntoView) {
            Element.prototype.scrollIntoView = vi.fn();
        }
        mockNavigate.mockReset();
        mockGet.mockReset();
        mockPost.mockReset();
        mockPatch.mockReset();
        mockPut.mockReset();
        mockDelete.mockReset();

        mockGet.mockImplementation((path: string) => {
            if (path === '/api/v1/admin/series/{id}') {
                return Promise.resolve({ data: { data: makeSeries() } });
            }
            if (path === '/api/v1/admin/categories') {
                return Promise.resolve({
                    data: {
                        data: [
                            {
                                id: '019535d8-7e21-7f00-a111-2233445566aa',
                                slug: 'rust',
                                name: 'Rust',
                                description: null,
                                post_count: 5,
                                revision_count: 5,
                                created_at: '2026-11-01T00:00:00+08:00',
                                updated_at: '2026-11-01T00:00:00+08:00',
                            },
                        ],
                    },
                });
            }
            return Promise.resolve({ data: { data: [] } });
        });
    });

    it('preserves local outline on 409 series_edit_conflict and allows resubmitting with latest version', async () => {
        const user = userEvent.setup();
        renderWorkbench();

        await screen.findByText('Rust 线程与作用域线程');
        // 目录短标题与分组编辑区默认折叠
        expect(screen.queryByDisplayValue('线程基础')).not.toBeInTheDocument();
        await user.click(
            screen.getByRole('button', { name: /编辑 Rust 线程与作用域线程 的目录设置/ }),
        );

        const displayTitleInput = await screen.findByDisplayValue('线程基础');
        await user.clear(displayTitleInput);
        await user.type(displayTitleInput, '线程与作用域');

        let latestSeries = makeSeries({ edit_version: 4 });
        mockGet.mockImplementation((path: string) => {
            if (path === '/api/v1/admin/series/{id}') {
                return Promise.resolve({ data: { data: latestSeries } });
            }
            return Promise.resolve({ data: { data: [] } });
        });
        mockPut.mockRejectedValueOnce(new ApiError({ status: 409, code: 'series_edit_conflict' }));

        await user.click(screen.getByRole('button', { name: '保存目录' }));

        await waitFor(() => {
            expect(screen.getByText(/目录保存冲突/)).toBeInTheDocument();
        });
        // 本地输入未被覆盖
        expect(screen.getByDisplayValue('线程与作用域')).toBeInTheDocument();

        const updatedSeries = makeSeries({ edit_version: 5 });
        mockPut.mockResolvedValueOnce({
            data: { data: { series: updatedSeries, publication: null } },
        });
        await user.click(
            screen.getByRole('button', { name: /以最新版本号（v4）重新提交本地目录/ }),
        );

        await waitFor(() => {
            expect(mockPut).toHaveBeenLastCalledWith(
                '/api/v1/admin/series/{id}/outline',
                expect.objectContaining({
                    body: expect.objectContaining({
                        expected_edit_version: 4,
                    }),
                }),
            );
        });
    });

    it('recovers from quick-create post succeeding when append fails without creating duplicate posts', async () => {
        const user = userEvent.setup();
        renderWorkbench();

        await screen.findByRole('heading', { name: 'Rust 并发编程' });
        await user.click(screen.getByRole('button', { name: '在此系列新建文章' }));

        await user.type(screen.getByLabelText(/文章标题/), '通道与消息传递');
        await user.type(screen.getByLabelText(/文章 Slug/), 'rust-channels');

        const createdPost: Schemas['Post'] = {
            id: POST_ID_NEW,
            slug: 'rust-channels',
            slug_locked: false,
            author: null,
            current_revision_id: '019535da-0000-7000-8000-000000000099',
            current_revision: {
                id: '019535da-0000-7000-8000-000000000099',
                post_id: POST_ID_NEW,
                revision_no: 1,
                title: '通道与消息传递',
                excerpt: null,
                markdown_body: '',
                rendered_html: '',
                headings: [],
                categories: [],
                tags: [],
                media_ids: [],
                cover_media_id: null,
                is_featured: false,
                seo_title: null,
                seo_description: null,
                created_by: null,
                created_at: '2026-11-26T12:00:00+08:00',
            },
            publication: {
                published_revision_id: null,
                published_at: null,

                first_published_at: null,
                publication_version: 0,
            },
            created_at: '2026-11-26T12:00:00+08:00',
            updated_at: '2026-11-26T12:00:00+08:00',
            series: null,
        };

        let postCreateCalls = 0;
        let appendCalls = 0;
        mockPost.mockImplementation((path: string) => {
            if (path === '/api/v1/admin/posts') {
                postCreateCalls += 1;
                return Promise.resolve({ data: { data: createdPost } });
            }
            if (path === '/api/v1/admin/series/{id}/items') {
                appendCalls += 1;
                if (appendCalls === 1) {
                    return Promise.reject(
                        new ApiError({ status: 409, code: 'series_edit_conflict' }),
                    );
                }
                return Promise.resolve({
                    data: {
                        data: {
                            item: {
                                id: '019535e2-0000-7000-8000-000000000099',
                                post_id: POST_ID_NEW,
                                display_title: null,
                                section_id: null,
                                position: 0,
                                title: '通道与消息传递',
                                slug: 'rust-channels',
                                current_revision_id: createdPost.current_revision_id,
                                published_revision_id: null,
                            },
                            edit_version: 4,
                            publication: null,
                        },
                    },
                });
            }
            return Promise.resolve({ data: {} });
        });

        await user.click(screen.getByRole('button', { name: '创建并加入系列' }));

        // 文章已创建并保留编辑入口，按钮切换为「重试加入系列」
        await waitFor(() => {
            expect(screen.getByText(/草稿已创建：通道与消息传递/)).toBeInTheDocument();
        });
        expect(screen.getByRole('link', { name: /打开编辑页/ })).toHaveAttribute(
            'href',
            `/posts/${POST_ID_NEW}/edit`,
        );

        await user.click(screen.getByRole('button', { name: '重试加入系列' }));

        await waitFor(() => {
            expect(appendCalls).toBe(2);
        });
        // 重试时绝不重复创建文章
        expect(postCreateCalls).toBe(1);
    });

    it('requires withdrawal before deleting a public series', async () => {
        mockGet.mockImplementation((path: string) => {
            if (path === '/api/v1/admin/series/{id}') {
                return Promise.resolve({
                    data: {
                        data: makeSeries({
                            published_at: '2026-11-26T10:00:00+08:00',
                            is_live: true,
                        }),
                    },
                });
            }
            return Promise.resolve({ data: { data: [] } });
        });

        renderWorkbench();
        await screen.findByText('访客可以查看专栏页和已发布的成员文章。');
        expect(screen.getByRole('button', { name: /删除系列/ })).toBeDisabled();
    });

    it('shows status metadata under series description and opens basic info inside a drawer on demand', async () => {
        const user = userEvent.setup();
        renderWorkbench();

        await screen.findByRole('heading', { name: 'Rust 并发编程' });
        expect(screen.getByText('从线程到异步运行时')).toBeInTheDocument();
        expect(screen.getByText('线上状态：未在线')).toBeInTheDocument();
        // 基础信息表单默认收起在抽屉内
        expect(screen.queryByLabelText(/^专栏标题/)).not.toBeInTheDocument();

        await user.click(screen.getByRole('button', { name: /基础信息/ }));
        expect(screen.getByRole('dialog')).toBeInTheDocument();
        expect(screen.getByLabelText(/^专栏标题/)).toHaveValue('Rust 并发编程');
    });

    it('publishes an unpublished outline post directly from the workbench row and supports batch publish', async () => {
        const user = userEvent.setup();
        const POST_ID_DRAFT = '019535d9-0000-7000-8000-000000000002';
        const REV_ID_DRAFT = '019535da-0000-7000-8000-000000000002';
        const seriesWithDraft = makeSeries({
            item_count: 2,
            published_post_count: 1,
            items: [
                {
                    id: '019535e2-0000-7000-8000-000000000002',
                    post_id: POST_ID_DRAFT,
                    display_title: null,
                    section_id: null,
                    position: 0,
                    title: 'Async 运行时入门',
                    slug: 'async-runtime',
                    current_revision_id: REV_ID_DRAFT,
                    published_revision_id: null,
                },
            ],
        });

        mockGet.mockImplementation(
            (path: string, opts?: { params?: { path?: { id?: string } } }) => {
                if (path === '/api/v1/admin/series/{id}') {
                    return Promise.resolve({ data: { data: seriesWithDraft } });
                }
                if (
                    path === '/api/v1/admin/posts/{id}' &&
                    opts?.params?.path?.id === POST_ID_DRAFT
                ) {
                    return Promise.resolve({
                        data: {
                            data: {
                                id: POST_ID_DRAFT,
                                slug: 'async-runtime',
                                current_revision_id: REV_ID_DRAFT,
                                publication: {
                                    publication_version: 0,
                                    published_revision_id: null,
                                },
                            },
                        },
                    });
                }
                return Promise.resolve({ data: { data: [] } });
            },
        );

        mockPut.mockResolvedValue({
            data: {
                data: {
                    generation: 8,
                    publication_version: 1,
                },
            },
        });

        renderWorkbench();

        await screen.findByText('Async 运行时入门');
        // 大纲顶栏显示一键发布按钮（1 篇待发布草稿）
        expect(screen.getByRole('button', { name: /一键发布 \(1\)/ })).toBeInTheDocument();

        // 点击行内发布按钮直接发布文章
        await user.click(screen.getByRole('button', { name: '发布 Async 运行时入门' }));

        await waitFor(() => {
            expect(mockPut).toHaveBeenCalledWith('/api/v1/admin/posts/{id}/publication', {
                params: { path: { id: POST_ID_DRAFT } },
                body: {
                    revision_id: REV_ID_DRAFT,
                    expected_publication_version: 0,
                },
            });
        });
    });
});
