import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PostEditorPage } from './post-editor-page';
import { ToastProvider } from '@/components/ui/toast';
import { PERMISSIONS } from '@/lib/permissions';
import { ApiError } from '@/lib/errors';

const mockNavigate = vi.fn();
const mockGet = vi.fn();
const mockPost = vi.fn();
const mockPut = vi.fn();
let permissions: string[];

vi.mock('@tanstack/react-router', () => ({
    Link: ({ children, to, ...props }: { children: ReactNode; to: string }) => (
        <a href={to} {...props}>
            {children}
        </a>
    ),
    useNavigate: () => mockNavigate,
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
        GET: (...args: unknown[]) => mockGet(...args),
        POST: (...args: unknown[]) => mockPost(...args),
        PATCH: vi.fn(),
        PUT: (...args: unknown[]) => mockPut(...args),
        DELETE: vi.fn(),
    },
    uploadMediaFile: vi.fn(),
    unwrapResponse: async <T,>(promise: Promise<{ data: T }>) => {
        const res = await promise;
        return res.data;
    },
}));

vi.mock('./tiptap-markdown-editor', () => ({
    TiptapMarkdownEditor: ({
        value,
        onChange,
    }: {
        value: string;
        onChange: (nextValue: string) => void;
    }) => (
        <textarea
            aria-label="Markdown 正文编辑器"
            value={value}
            onChange={(event) => onChange(event.target.value)}
        />
    ),
}));

function renderEditor(postId?: string, initialCategoryId?: string) {
    const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    return render(
        <QueryClientProvider client={queryClient}>
            <ToastProvider>
                <PostEditorPage postId={postId} initialCategoryId={initialCategoryId} />
            </ToastProvider>
        </QueryClientProvider>,
    );
}

describe('PostEditorPage', () => {
    beforeEach(() => {
        permissions = [
            PERMISSIONS.POSTS_CREATE,
            PERMISSIONS.POSTS_MANAGE,
            PERMISSIONS.POSTS_PUBLISH,
        ];
        mockNavigate.mockReset();
        mockGet.mockReset();
        mockPost.mockReset();
        mockPut.mockReset();
        mockGet.mockResolvedValue({ data: { data: [] } });
    });

    it('defaults to focus mode, hides revision status strip, and separates back button from title', async () => {
        const user = userEvent.setup();
        renderEditor();

        // 默认专注模式：专注按钮激活，预览区域处于桌面隐藏状态
        const modeGroup = screen.getByRole('group', { name: '编辑视图模式' });
        const focusButton = within(modeGroup).getByRole('button', { name: '专注' });
        const previewButton = within(modeGroup).getByRole('button', { name: '预览' });
        expect(focusButton).toHaveAttribute('aria-pressed', 'true');
        expect(previewButton).toHaveAttribute('aria-pressed', 'false');
        expect(screen.getByLabelText('预览')).toHaveClass('lg:hidden');

        // 可切换至预览模式并切回专注模式
        await user.click(previewButton);
        expect(previewButton).toHaveAttribute('aria-pressed', 'true');
        expect(screen.getByLabelText('预览')).not.toHaveClass('lg:hidden');

        await user.click(focusButton);
        expect(focusButton).toHaveAttribute('aria-pressed', 'true');
        expect(screen.getByLabelText('预览')).toHaveClass('lg:hidden');

        // 新建文章页不展示三种修订状态条
        expect(screen.queryByText('当前编辑修订')).not.toBeInTheDocument();
        expect(screen.queryByText('期望公开修订')).not.toBeInTheDocument();
        expect(screen.queryByText('当前线上修订')).not.toBeInTheDocument();

        // 文章设置抽屉默认关闭
        expect(screen.queryByLabelText(/Slug/)).not.toBeInTheDocument();

        // 返回按钮独立位于操作栏上方，文章标题位于写作区，不在同一块
        const topBar = screen.getByRole('banner', { name: '文章操作栏' });
        const editorSection = screen.getByLabelText('正文编辑器');
        expect(screen.getByRole('button', { name: '返回文章列表' })).toBeInTheDocument();
        expect(
            within(topBar).queryByRole('button', { name: '返回文章列表' }),
        ).not.toBeInTheDocument();
        expect(within(topBar).queryByLabelText('文章标题')).not.toBeInTheDocument();
        expect(within(editorSection).getByLabelText('文章标题')).toBeInTheDocument();
    });

    it('navigates directly on back when empty, and requires confirmation when title or body has content', async () => {
        const user = userEvent.setup();
        renderEditor();

        // 无内容时点击返回直接跳转，不弹窗
        await user.click(screen.getByRole('button', { name: '返回文章列表' }));
        expect(mockNavigate).toHaveBeenCalledWith({ to: '/posts' });
        expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();

        mockNavigate.mockClear();

        // 输入标题后点击返回，应弹出二次确认
        await user.type(screen.getByLabelText('文章标题'), '测试标题');
        await user.click(screen.getByRole('button', { name: '返回文章列表' }));

        const confirmDialog = screen.getByRole('alertdialog');
        expect(within(confirmDialog).getByText('放弃当前草稿并返回？')).toBeInTheDocument();
        expect(mockNavigate).not.toHaveBeenCalled();

        // 点击继续编辑关闭弹窗并留在页面
        await user.click(within(confirmDialog).getByRole('button', { name: '继续编辑' }));
        expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
        expect(mockNavigate).not.toHaveBeenCalled();

        // 清空标题、仅输入正文后点击返回，同样需要二次确认
        await user.clear(screen.getByLabelText('文章标题'));
        await user.type(screen.getByLabelText('Markdown 正文编辑器'), '一段正文内容');
        await user.click(screen.getByRole('button', { name: '返回文章列表' }));

        const secondDialog = screen.getByRole('alertdialog');
        await user.click(within(secondDialog).getByRole('button', { name: '放弃并返回' }));
        expect(mockNavigate).toHaveBeenCalledWith({ to: '/posts' });
    });

    const revision = {
        id: 'draft-1',
        title: '草稿',
        excerpt: null,
        markdown_body: '',
        categories: [],
        tags: [],
        revision_no: 1,
        created_at: '2026-11-01T10:00:00Z',
    };
    const post = {
        id: 'post-1',
        slug: 'post',
        current_revision_id: revision.id,
        current_revision: revision,
        slug_locked: false,
        publication: {
            published_revision_id: null,

            publication_version: 0,
        },
    };

    function mockArticle(read: () => unknown = () => post) {
        mockGet.mockImplementation(async (path: string) => ({
            data: { data: path === '/api/v1/admin/posts/{id}' ? read() : [] },
        }));
    }

    it('lets authors save drafts and hides publication actions', async () => {
        permissions = [PERMISSIONS.POSTS_CREATE, PERMISSIONS.POSTS_EDIT_OWN];
        renderEditor();
        expect(screen.getByRole('button', { name: '保存草稿' })).toBeEnabled();
        expect(screen.queryByRole('button', { name: '发布文章' })).not.toBeInTheDocument();
    });

    it('saves a new draft with a custom link without publishing', async () => {
        const user = userEvent.setup();
        mockPost.mockResolvedValue({ data: { data: post } });
        renderEditor();
        await user.type(screen.getByLabelText('文章标题'), '第一篇文章');
        await user.click(screen.getByRole('button', { name: /文章设置/ }));
        await user.type(screen.getByLabelText('Slug'), 'my-first-post');
        await user.click(
            within(screen.getByRole('dialog')).getByRole('button', { name: '保存草稿' }),
        );
        await waitFor(() =>
            expect(mockNavigate).toHaveBeenCalledWith({
                to: '/posts/$postId/edit',
                params: { postId: 'post-1' },
            }),
        );
        expect(mockPost).toHaveBeenCalledWith(
            '/api/v1/admin/posts',
            expect.objectContaining({
                body: expect.objectContaining({ title: '第一篇文章', slug: 'my-first-post' }),
            }),
        );
        expect(mockPut).not.toHaveBeenCalled();
    });

    it('creates and publishes a new article in one click with an automatic slug', async () => {
        const user = userEvent.setup();
        mockPost.mockImplementation(async (_path, { body }) => ({
            data: { data: { ...post, slug: body.slug } },
        }));
        mockPut.mockResolvedValue({
            data: { data: { generation: 1 } },
        });
        mockArticle(() => ({
            ...post,
            publication: { ...post.publication, published_revision_id: revision.id },
        }));
        renderEditor();
        await user.type(screen.getByLabelText('文章标题'), '第一篇文章');
        await user.type(screen.getByLabelText('Markdown 正文编辑器'), '正文');
        await user.click(screen.getByRole('button', { name: '发布文章' }));
        await waitFor(() =>
            expect(mockNavigate).toHaveBeenCalledWith({
                to: '/posts/$postId/edit',
                params: { postId: 'post-1' },
            }),
        );
        expect(mockPost).toHaveBeenCalledWith(
            '/api/v1/admin/posts',
            expect.objectContaining({
                body: expect.objectContaining({
                    title: '第一篇文章',
                    markdown_body: '正文',
                    slug: expect.stringMatching(/^post-[a-f0-9]+$/),
                }),
            }),
        );
        expect(mockPut).toHaveBeenCalledWith('/api/v1/admin/posts/{id}/publication', {
            params: { path: { id: 'post-1' } },
            body: { revision_id: revision.id, expected_publication_version: 0 },
        });
        expect(screen.getByText('文章已上线')).toBeInTheDocument();
    });

    it('opens the created draft for retry when publication fails', async () => {
        const user = userEvent.setup();
        mockPost.mockImplementation(async (_path, { body }) => ({
            data: { data: { ...post, slug: body.slug } },
        }));
        mockPut.mockRejectedValue(new Error('publication failed'));
        renderEditor();
        await user.type(screen.getByLabelText('文章标题'), '新文章');
        await user.click(screen.getByRole('button', { name: '发布文章' }));
        await waitFor(() =>
            expect(mockNavigate).toHaveBeenCalledWith({
                to: '/posts/$postId/edit',
                params: { postId: post.id },
            }),
        );
        expect(mockPost).toHaveBeenCalledTimes(1);
        expect(screen.queryByText('文章已上线')).not.toBeInTheDocument();
    });

    it('waits for an existing save and publishes the exact revision at click time', async () => {
        const user = userEvent.setup();
        mockArticle();
        let finishFirstSave!: (value: unknown) => void;
        mockPost.mockImplementationOnce(
            () =>
                new Promise((resolve) => {
                    finishFirstSave = resolve;
                }),
        );
        mockPost.mockResolvedValueOnce({ data: { data: { ...revision, id: 'draft-3' } } });
        mockPut.mockResolvedValue({
            data: { data: { generation: 1 } },
        });
        renderEditor('post-1');
        const title = await screen.findByDisplayValue('草稿');
        await user.type(title, '一');
        await user.click(screen.getByRole('button', { name: '保存文章' }));
        await user.type(title, '二');
        await user.click(screen.getByRole('button', { name: '发布文章' }));
        await user.type(title, '三');
        expect(mockPut).not.toHaveBeenCalled();
        finishFirstSave({ data: { data: { ...revision, id: 'draft-2' } } });
        await waitFor(() =>
            expect(mockPut).toHaveBeenCalledWith('/api/v1/admin/posts/{id}/publication', {
                params: { path: { id: 'post-1' } },
                body: { revision_id: 'draft-3', expected_publication_version: 0 },
            }),
        );
        expect(mockPost).toHaveBeenLastCalledWith(
            '/api/v1/admin/posts/{id}/revisions',
            expect.objectContaining({
                body: expect.objectContaining({
                    title: '草稿一二',
                    expected_revision_id: 'draft-2',
                }),
            }),
        );
        expect(title).toHaveValue('草稿一二三');
        expect(
            await screen.findByText('点击发布时的版本已上线，后续编辑仍在草稿中。'),
        ).toBeInTheDocument();
    });

    it('keeps local edits and stops publication after a revision conflict', async () => {
        const user = userEvent.setup();
        mockArticle();
        mockPost.mockRejectedValue(new ApiError({ status: 409, code: 'revision_conflict' }));
        renderEditor('post-1');
        const title = await screen.findByDisplayValue('草稿');
        await user.type(title, '新版');
        await user.click(screen.getByRole('button', { name: '发布文章' }));
        expect(await screen.findByText('有新版本，已停止自动保存。')).toBeInTheDocument();
        expect(screen.getByRole('button', { name: '发布文章' })).toBeDisabled();
        expect(title).toHaveValue('草稿新版');
        expect(mockPut).not.toHaveBeenCalled();
    });

    it('requires review when another editor replaces the revision just before publication', async () => {
        const user = userEvent.setup();
        mockArticle();
        mockPut.mockRejectedValue(
            new ApiError({
                status: 422,
                code: 'validation_error',
                details: [
                    {
                        field: 'revision_id',
                        code: 'invalid_value',
                        message: 'revision was replaced',
                    },
                ],
            }),
        );
        renderEditor('post-1');
        const title = await screen.findByDisplayValue('草稿');
        await user.click(screen.getByRole('button', { name: '发布文章' }));
        expect(await screen.findByText('有新版本，已停止自动保存。')).toBeInTheDocument();
        expect(title).toHaveValue('草稿');
        expect(screen.getByRole('button', { name: '发布文章' })).toBeDisabled();
        expect(mockPut).toHaveBeenCalledTimes(1);
    });

    it('pre-selects initialCategoryId in create mode', async () => {
        const user = userEvent.setup();
        mockGet.mockImplementation((path: string) => {
            if (path === '/api/v1/admin/categories') {
                return Promise.resolve({
                    data: {
                        data: [
                            {
                                id: 'cat-backend',
                                slug: 'backend',
                                name: '后端架构',
                                description: null,
                                post_count: 3,
                                revision_count: 3,
                                created_at: '2026-11-01T00:00:00Z',
                                updated_at: '2026-11-01T00:00:00Z',
                            },
                        ],
                    },
                });
            }
            return Promise.resolve({ data: { data: [] } });
        });

        renderEditor(undefined, 'cat-backend');

        await user.click(screen.getByRole('button', { name: /文章设置/ }));
        const categoryBtn = await screen.findByRole('button', { name: '后端架构' });
        expect(categoryBtn).toHaveAttribute('aria-pressed', 'true');
    });
});
