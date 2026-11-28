import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { createRef } from 'react';
import { describe, expect, it, vi } from 'vitest';
import {
    TiptapMarkdownEditor,
    type TiptapMarkdownEditorHandle,
    type UploadedEditorImage,
} from './tiptap-markdown-editor';

vi.mock('@/components/media/authenticated-media', () => ({
    AuthenticatedImage: ({ mediaId, alt }: { mediaId: string | null | undefined; alt: string }) => (
        <img data-testid="auth-media-img" data-media-id={mediaId ?? ''} alt={alt} />
    ),
}));

describe('TiptapMarkdownEditor', () => {
    it('parses initial markdown content and renders formatting toolbar', async () => {
        render(
            <TiptapMarkdownEditor
                value={'## 小节标题\n\n包含 **粗体** 与 `code` 的正文。'}
                onChange={vi.fn()}
                onRequestInsertMedia={vi.fn()}
            />,
        );

        await waitFor(() => {
            expect(screen.getByRole('heading', { level: 2, name: '小节标题' })).toBeInTheDocument();
        });
        expect(screen.getByText('粗体').tagName).toBe('STRONG');
        expect(screen.getByText('code').tagName).toBe('CODE');
        expect(screen.getByRole('button', { name: '插入媒体' })).toBeInTheDocument();
    });

    it('supports both inline and standalone images and preserves user-defined alt text', async () => {
        const handleChange = vi.fn();
        const ref = createRef<TiptapMarkdownEditorHandle>();
        const mediaIdA = '019535d7-1111-7222-8333-444455556666';
        const mediaIdB = '019535d7-2222-7222-8333-444455556666';

        render(
            <TiptapMarkdownEditor
                ref={ref}
                value={`前文 ![](media:${mediaIdA}) 后文`}
                onChange={handleChange}
            />,
        );

        // 同一段落内支持图文混排（不强行拆行）
        await waitFor(() => {
            const img = screen.getByTestId('auth-media-img');
            expect(img).toHaveAttribute('data-media-id', mediaIdA);
            expect(img).toHaveAttribute('alt', '');
        });

        // 选中图片后可由用户自行编辑 Alt 说明与颜色
        const user = userEvent.setup();
        fireEvent.click(screen.getByTestId('auth-media-img'));

        const altInput = await screen.findByLabelText('Alt');
        await user.type(altInput, '架构示意图');
        await user.click(screen.getByRole('button', { name: '颜色：森绿' }));
        await user.click(screen.getByRole('button', { name: '确定' }));

        await waitFor(() => {
            const lastMarkdown = handleChange.mock.calls.at(-1)?.[0] as string;
            expect(lastMarkdown).toContain(
                `前文 ![架构示意图](media:${mediaIdA} "alt-color:#137a4d") 后文`,
            );
        });

        // 图注展示在图片下方且水平居中，并应用所选字体颜色
        const caption = screen.getByText('架构示意图');
        expect(caption).toHaveClass('text-center');
        expect(caption).toHaveStyle({ color: '#137a4d' });

        // 插入第二张无 Alt 的图片时，不会使用文件名填充 Alt
        act(() => {
            ref.current?.insertImage({ src: `media:${mediaIdB}`, alt: '' });
        });

        await waitFor(() => {
            const lastMarkdown = handleChange.mock.calls.at(-1)?.[0] as string;
            expect(lastMarkdown).toContain(`![](media:${mediaIdB})`);
        });
    });

    it('syncs externally updated markdown content without triggering onChange loop', async () => {
        const handleChange = vi.fn();
        const { rerender } = render(
            <TiptapMarkdownEditor value="初始段落" onChange={handleChange} />,
        );

        await waitFor(() => {
            expect(screen.getByText('初始段落')).toBeInTheDocument();
        });

        rerender(<TiptapMarkdownEditor value="## 更新后的最新修订" onChange={handleChange} />);

        await waitFor(() => {
            expect(
                screen.getByRole('heading', { level: 2, name: '更新后的最新修订' }),
            ).toBeInTheDocument();
        });
        expect(handleChange).not.toHaveBeenCalled();
    });

    it('uploads pasted images automatically inline at the cursor without filename alt', async () => {
        const handleChange = vi.fn();
        const mediaId = '019535d7-aaaa-7222-8333-444455556666';
        let resolveUpload: ((result: UploadedEditorImage | null) => void) | undefined;
        const onUploadImage = vi.fn(
            () =>
                new Promise<UploadedEditorImage | null>((resolve) => {
                    resolveUpload = resolve;
                }),
        );

        render(
            <TiptapMarkdownEditor
                value="第一段正文"
                onChange={handleChange}
                onUploadImage={onUploadImage}
            />,
        );

        const editable = await screen.findByLabelText('Markdown 正文编辑器');
        const pastedFile = new File(
            [new Uint8Array([137, 80, 78, 71])],
            '4917f4e0525684cd3c31f6ad81bfa3dd.png',
            {
                type: 'image/png',
            },
        );

        fireEvent.paste(editable, {
            clipboardData: {
                getData: () => '',
                items: [
                    {
                        kind: 'file',
                        type: 'image/png',
                        getAsFile: () => pastedFile,
                    },
                ],
                files: [pastedFile],
            },
        });

        expect(onUploadImage).toHaveBeenCalledWith(pastedFile);
        expect(
            screen.getByText('上传中：4917f4e0525684cd3c31f6ad81bfa3dd.png'),
        ).toBeInTheDocument();

        act(() => {
            resolveUpload?.({ src: `media:${mediaId}`, alt: '' });
        });

        await waitFor(() => {
            expect(
                screen.queryByText('上传中：4917f4e0525684cd3c31f6ad81bfa3dd.png'),
            ).not.toBeInTheDocument();
            expect(screen.getByTestId('auth-media-img')).toHaveAttribute('data-media-id', mediaId);
        });

        const lastMarkdown = handleChange.mock.calls.at(-1)?.[0] as string;
        expect(lastMarkdown).toContain(`![](media:${mediaId})`);
        expect(lastMarkdown).not.toContain('4917f4e0525684cd3c31f6ad81bfa3dd.png');
        // 粘贴在已有段落内时不强制拆成独立段落
        expect(lastMarkdown).not.toContain('\n\n');
    });

    it('cleans up upload placeholder when pasted image upload fails', async () => {
        const handleChange = vi.fn();
        const onUploadImage = vi.fn().mockResolvedValue(null);

        render(
            <TiptapMarkdownEditor
                value="保留正文"
                onChange={handleChange}
                onUploadImage={onUploadImage}
            />,
        );

        const editable = await screen.findByLabelText('Markdown 正文编辑器');
        const pastedFile = new File([new Uint8Array([1, 2, 3])], 'bad.png', {
            type: 'image/png',
        });

        fireEvent.paste(editable, {
            clipboardData: {
                getData: () => '',
                files: [pastedFile],
            },
        });

        await waitFor(() => {
            expect(onUploadImage).toHaveBeenCalledWith(pastedFile);
            expect(screen.queryByText('上传中：bad.png')).not.toBeInTheDocument();
        });

        expect(screen.queryByTestId('auth-media-img')).not.toBeInTheDocument();
        expect(handleChange).not.toHaveBeenCalled();
    });

    it('renders code block language selector, highlights code with Shiki, and serializes markdown fence language', async () => {
        const handleChange = vi.fn();
        const user = userEvent.setup();

        render(
            <TiptapMarkdownEditor
                value={'```ts\nconst answer = 42;\n```'}
                onChange={handleChange}
            />,
        );

        const select = (await screen.findByLabelText('代码块语言')) as HTMLSelectElement;
        expect(select.value).toBe('typescript');

        // 等待 Shiki 按需加载 TypeScript 语法并注入 ProseMirror 行内高亮装饰
        await waitFor(() => {
            const styledToken = document.querySelector(
                '.silva-code-block-pre code span[style*="color"]',
            );
            expect(styledToken).not.toBeNull();
        });

        // 切换语言为 Rust 后应同步序列化为 ```rust
        await user.selectOptions(select, 'rust');
        await waitFor(() => {
            const lastMarkdown = handleChange.mock.calls.at(-1)?.[0] as string;
            expect(lastMarkdown).toContain('```rust\nconst answer = 42;\n```');
        });

        // 切换回纯文本后应移除语言标识
        await user.selectOptions(select, '');
        await waitFor(() => {
            const lastMarkdown = handleChange.mock.calls.at(-1)?.[0] as string;
            expect(lastMarkdown).toContain('```\nconst answer = 42;\n```');
        });
    });

    it('preserves custom unknown code block languages in the language selector', async () => {
        render(
            <TiptapMarkdownEditor
                value={'```zig\nconst std = @import("std");\n```'}
                onChange={vi.fn()}
            />,
        );

        const select = (await screen.findByLabelText('代码块语言')) as HTMLSelectElement;
        expect(select.value).toBe('zig');
    });
});
