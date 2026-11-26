import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PostEditorPage } from './post-editor-page';
import { ToastProvider } from '@/components/ui/toast';
import { PERMISSIONS } from '@/lib/permissions';

const mockNavigate = vi.fn();
const mockGet = vi.fn();
const mockPost = vi.fn();

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
            permissions: [
                PERMISSIONS.POSTS_CREATE,
                PERMISSIONS.POSTS_MANAGE,
                PERMISSIONS.POSTS_SUBMIT_OWN,
                PERMISSIONS.POSTS_PUBLISH,
            ],
        },
    }),
}));

vi.mock('@/lib/api', () => ({
    apiClient: {
        GET: (...args: unknown[]) => mockGet(...args),
        POST: (...args: unknown[]) => mockPost(...args),
        PATCH: vi.fn(),
        PUT: vi.fn(),
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

function renderEditor(postId?: string) {
    const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    return render(
        <QueryClientProvider client={queryClient}>
            <ToastProvider>
                <PostEditorPage postId={postId} />
            </ToastProvider>
        </QueryClientProvider>,
    );
}

describe('PostEditorPage (create mode)', () => {
    beforeEach(() => {
        mockNavigate.mockReset();
        mockGet.mockReset();
        mockPost.mockReset();
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

        // 返回按钮位于顶栏操作区，文章标题位于写作区，不在同一块
        const topBar = screen.getByRole('banner', { name: '文章操作栏' });
        const editorSection = screen.getByLabelText('正文编辑器');
        expect(within(topBar).getByRole('button', { name: '返回文章列表' })).toBeInTheDocument();
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

    it('supports manual settings drawer open and auto-opens settings drawer on create when slug is missing', async () => {
        const user = userEvent.setup();
        mockPost.mockResolvedValue({
            data: {
                data: {
                    id: 'post-1',
                    slug: 'my-first-post',
                    slug_locked: false,
                    current_revision_id: 'rev-1',
                    current_revision: {
                        id: 'rev-1',
                        revision_no: 1,
                        title: '第一篇文章',
                        excerpt: null,
                        markdown_body: 'hello',
                        categories: [],
                        tags: [],
                        cover_media_id: null,
                        is_featured: false,
                        seo_title: null,
                        seo_description: null,
                        review: null,
                        created_at: new Date().toISOString(),
                    },
                    publication: {
                        desired_published_revision_id: null,
                        live_revision_id: null,
                        publication_version: 1,
                    },
                },
            },
        });

        renderEditor();

        // 可手动打开并关闭文章设置抽屉
        await user.click(screen.getByRole('button', { name: /文章设置/ }));
        expect(screen.getByRole('heading', { name: '文章设置' })).toBeInTheDocument();
        await user.click(screen.getByRole('button', { name: '关闭抽屉' }));
        expect(screen.queryByRole('heading', { name: '文章设置' })).not.toBeInTheDocument();

        // 仅填写标题未填 Slug，点击「创建文章」自动打开设置抽屉并提示填写 Slug
        await user.type(screen.getByLabelText('文章标题'), '第一篇文章');
        await user.click(screen.getByRole('button', { name: '创建文章' }));

        expect(screen.getByRole('heading', { name: '文章设置' })).toBeInTheDocument();
        expect(screen.getByText('请填写 Slug 后再创建文章')).toBeInTheDocument();
        expect(mockPost).not.toHaveBeenCalled();

        // 在抽屉内填写 Slug 并点击抽屉底部的创建文章
        await user.type(screen.getByLabelText(/Slug/), 'my-first-post');
        const drawer = screen.getByRole('dialog');
        await user.click(within(drawer).getByRole('button', { name: '创建文章' }));

        await waitFor(() => {
            expect(mockPost).toHaveBeenCalledWith(
                '/api/v1/admin/posts',
                expect.objectContaining({
                    body: expect.objectContaining({
                        title: '第一篇文章',
                        slug: 'my-first-post',
                    }),
                }),
            );
        });
        expect(mockNavigate).toHaveBeenCalledWith({
            to: '/posts/$postId/edit',
            params: { postId: 'post-1' },
        });
    });
});
