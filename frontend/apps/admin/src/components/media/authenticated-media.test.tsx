import { render, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { SafeHtmlPreview } from './authenticated-media';
import { renderSafeMarkdown } from '@/lib/markdown';

describe('SafeHtmlPreview code block Shiki highlighting', () => {
    it('wraps code blocks with language header and highlights tokens safely via Shiki', async () => {
        const html = renderSafeMarkdown(
            '```rust\nfn main() {\n    println!("hello");\n}\n```\n\n```c++\nint x = 1;\n```',
        );

        const { container } = render(<SafeHtmlPreview html={html} />);

        await waitFor(() => {
            const headers = Array.from(container.querySelectorAll('.silva-code-block-header')).map(
                (el) => el.textContent,
            );
            expect(headers).toEqual(['Rust', 'C++']);

            const highlightedBlocks = container.querySelectorAll('pre.silva-shiki');
            expect(highlightedBlocks).toHaveLength(2);
            expect(
                container.querySelector('pre.silva-shiki code span[style*="color"]'),
            ).not.toBeNull();
        });
    });

    it('renders plain text code blocks without failing when no language is specified', () => {
        const html = renderSafeMarkdown('```\nplain line 1\nplain line 2\n```');
        const { container } = render(<SafeHtmlPreview html={html} />);

        expect(container.querySelector('.silva-code-block-header')?.textContent).toBe('纯文本');
        expect(container.querySelector('pre code')?.textContent).toContain('plain line 1');
    });
});
