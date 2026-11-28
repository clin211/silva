import { describe, expect, it } from 'vitest';
import {
    getCodeLanguageLabel,
    isShikiLanguageLoaded,
    resolveShikiLanguage,
    tokenizeCodeAsync,
    tokenizeCodeSync,
    tokenToInlineStyle,
} from './shiki';

describe('shiki on-demand highlighter', () => {
    it('resolves language aliases and formats labels cleanly', () => {
        expect(resolveShikiLanguage('ts')).toBe('typescript');
        expect(resolveShikiLanguage('C++')).toBe('cpp');
        expect(resolveShikiLanguage('c#')).toBe('csharp');
        expect(resolveShikiLanguage('sh')).toBe('bash');
        expect(resolveShikiLanguage('yml')).toBe('yaml');
        expect(resolveShikiLanguage('plaintext')).toBeNull();
        expect(resolveShikiLanguage('')).toBeNull();
        expect(resolveShikiLanguage('unknown-custom-lang')).toBeNull();

        expect(getCodeLanguageLabel('ts')).toBe('TypeScript');
        expect(getCodeLanguageLabel('rust')).toBe('Rust');
        expect(getCodeLanguageLabel('c++')).toBe('C++');
        expect(getCodeLanguageLabel('')).toBe('纯文本');
        expect(getCodeLanguageLabel('zig')).toBe('zig');
    });

    it('loads a language grammar on demand and tokenizes with github-dark colors', async () => {
        const lines = await tokenizeCodeAsync('const count: number = 42;', 'ts');
        expect(lines).not.toBeNull();
        expect(lines).toHaveLength(1);
        expect(isShikiLanguageLoaded('typescript')).toBe(true);

        const tokens = lines![0]!;
        const constToken = tokens.find((token) => token.content === 'const');
        expect(constToken?.color).toMatch(/^#[0-9a-f]{6}$/i);
        expect(tokenToInlineStyle(constToken!)).toContain('color:');

        // 已加载的语言可直接同步分词
        const syncLines = tokenizeCodeSync('const x = 1;', 'typescript');
        expect(syncLines).not.toBeNull();
        expect(syncLines![0]!.length).toBeGreaterThan(1);
    });

    it('returns null gracefully for plaintext or unsupported languages', async () => {
        expect(await tokenizeCodeAsync('plain text', '')).toBeNull();
        expect(await tokenizeCodeAsync('plain text', 'unknown-lang')).toBeNull();
    });
});
