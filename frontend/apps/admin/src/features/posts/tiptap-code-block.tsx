import CodeBlock from '@tiptap/extension-code-block';
import type { Node as ProseMirrorNode } from '@tiptap/pm/model';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view';
import {
    NodeViewContent,
    NodeViewWrapper,
    ReactNodeViewRenderer,
    type NodeViewProps,
} from '@tiptap/react';
import { Check, ChevronDown, Copy } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
    CODE_LANGUAGE_OPTIONS,
    ensureShikiLanguage,
    getCodeLanguageLabel,
    isShikiLanguageLoaded,
    resolveShikiLanguage,
    tokenizeCodeSync,
    tokenToInlineStyle,
    type CodeLanguageOption,
} from '@/lib/shiki';

interface ShikiPluginMeta {
    refresh?: boolean;
}

const SHIKI_HIGHLIGHT_PLUGIN_KEY = new PluginKey<DecorationSet>('silvaShikiHighlight');

function buildShikiDecorations(doc: ProseMirrorNode): DecorationSet {
    const decorations: Decoration[] = [];

    doc.descendants((node, pos) => {
        if (node.type.name !== 'codeBlock') {
            return true;
        }

        const rawLanguage = typeof node.attrs.language === 'string' ? node.attrs.language : null;
        const code = node.textContent;
        if (!code || !rawLanguage) {
            return false;
        }

        const lines = tokenizeCodeSync(code, rawLanguage);
        if (!lines) {
            return false;
        }

        const maxLen = code.length;
        let cursor = 0;

        for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
            const tokens = lines[lineIndex] ?? [];
            for (const token of tokens) {
                const len = token.content.length;
                if (len <= 0) continue;

                const from = Math.min(cursor, maxLen);
                const to = Math.min(cursor + len, maxLen);
                cursor += len;

                if (from >= to) continue;
                const style = tokenToInlineStyle(token);
                if (!style) continue;

                decorations.push(
                    Decoration.inline(pos + 1 + from, pos + 1 + to, {
                        style,
                    }),
                );
            }

            if (lineIndex < lines.length - 1) {
                cursor += code.startsWith('\r\n', cursor) ? 2 : 1;
            }
        }

        return false;
    });

    return DecorationSet.create(doc, decorations);
}

function collectUnloadedLanguages(doc: ProseMirrorNode): string[] {
    const missing = new Set<string>();

    doc.descendants((node) => {
        if (node.type.name !== 'codeBlock') {
            return true;
        }

        const rawLanguage = typeof node.attrs.language === 'string' ? node.attrs.language : null;
        const resolved = resolveShikiLanguage(rawLanguage);
        if (resolved && !isShikiLanguageLoaded(resolved)) {
            missing.add(resolved);
        }

        return false;
    });

    return Array.from(missing);
}

function createShikiHighlightPlugin(): Plugin<DecorationSet> {
    return new Plugin<DecorationSet>({
        key: SHIKI_HIGHLIGHT_PLUGIN_KEY,
        state: {
            init(_config, instance) {
                return buildShikiDecorations(instance.doc);
            },
            apply(tr, oldSet) {
                const meta = tr.getMeta(SHIKI_HIGHLIGHT_PLUGIN_KEY) as ShikiPluginMeta | undefined;
                if (tr.docChanged || meta?.refresh) {
                    return buildShikiDecorations(tr.doc);
                }
                return oldSet;
            },
        },
        props: {
            decorations(state) {
                return this.getState(state);
            },
        },
        view(editorView) {
            let destroyed = false;
            const pending = new Set<string>();

            const loadMissingLanguages = (view: EditorView) => {
                const missing = collectUnloadedLanguages(view.state.doc);
                for (const lang of missing) {
                    if (pending.has(lang)) continue;
                    pending.add(lang);
                    void ensureShikiLanguage(lang)
                        .then((loaded) => {
                            if (!loaded || destroyed || view.isDestroyed) return;
                            view.dispatch(
                                view.state.tr
                                    .setMeta(SHIKI_HIGHLIGHT_PLUGIN_KEY, {
                                        refresh: true,
                                    } satisfies ShikiPluginMeta)
                                    .setMeta('addToHistory', false),
                            );
                        })
                        .finally(() => {
                            pending.delete(lang);
                        });
                }
            };

            loadMissingLanguages(editorView);

            return {
                update(view, prevState) {
                    if (view.state.doc !== prevState.doc) {
                        loadMissingLanguages(view);
                    }
                },
                destroy() {
                    destroyed = true;
                },
            };
        },
    });
}

function SilvaCodeBlockNodeView({ node, updateAttributes, editor }: NodeViewProps) {
    const rawLanguage = typeof node.attrs.language === 'string' ? node.attrs.language.trim() : '';
    const resolvedLanguage = resolveShikiLanguage(rawLanguage);
    const selectValue = resolvedLanguage ?? rawLanguage;
    const isEditable = editor.isEditable;

    const [copied, setCopied] = useState(false);
    const copyTimerRef = useRef<number | null>(null);

    useEffect(() => {
        return () => {
            if (copyTimerRef.current !== null) {
                window.clearTimeout(copyTimerRef.current);
            }
        };
    }, []);

    const options = useMemo<readonly CodeLanguageOption[]>(() => {
        if (!selectValue) return CODE_LANGUAGE_OPTIONS;
        const exists = CODE_LANGUAGE_OPTIONS.some((item) => item.value === selectValue);
        if (exists) return CODE_LANGUAGE_OPTIONS;
        return [
            CODE_LANGUAGE_OPTIONS[0]!,
            { value: selectValue, label: selectValue },
            ...CODE_LANGUAGE_OPTIONS.slice(1),
        ];
    }, [selectValue]);

    const handleCopy = async () => {
        try {
            if (!navigator.clipboard?.writeText) return;
            await navigator.clipboard.writeText(node.textContent);
            setCopied(true);
            if (copyTimerRef.current !== null) {
                window.clearTimeout(copyTimerRef.current);
            }
            copyTimerRef.current = window.setTimeout(() => {
                setCopied(false);
            }, 1600);
        } catch {
            // 剪贴板不可用时静默忽略
        }
    };

    return (
        <NodeViewWrapper
            as="div"
            className="silva-code-block"
            data-language={rawLanguage || 'plaintext'}
        >
            <div
                contentEditable={false}
                data-silva-code-controls=""
                className="silva-code-block-header"
            >
                {isEditable ? (
                    <label className="relative inline-flex items-center">
                        <span className="sr-only">代码块语言</span>
                        <select
                            aria-label="代码块语言"
                            value={selectValue}
                            onChange={(event) => {
                                const next = event.target.value;
                                updateAttributes({ language: next || null });
                            }}
                            className="h-6 appearance-none rounded-md border border-white/10 bg-white/5 pr-6 pl-2 font-code text-[11.5px] text-[#c9d1d9] transition-colors hover:border-white/20 hover:bg-white/10 focus:border-accent focus:outline-none"
                        >
                            {options.map((option) => (
                                <option
                                    key={option.value || 'plaintext'}
                                    value={option.value}
                                    className="bg-[#161b22] text-[#e6edf3]"
                                >
                                    {option.label}
                                </option>
                            ))}
                        </select>
                        <ChevronDown
                            aria-hidden="true"
                            className="pointer-events-none absolute right-1.5 h-3 w-3 text-[#8b949e]"
                        />
                    </label>
                ) : (
                    <span className="font-code text-[11.5px] font-medium text-[#8b949e]">
                        {getCodeLanguageLabel(rawLanguage)}
                    </span>
                )}

                <button
                    type="button"
                    aria-label="复制代码"
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => void handleCopy()}
                    className="inline-flex h-6 items-center gap-1 rounded-md px-2 text-[11.5px] text-[#8b949e] transition-colors hover:bg-white/10 hover:text-[#e6edf3]"
                >
                    {copied ? (
                        <>
                            <Check className="h-3 w-3 text-[#3fb950]" aria-hidden="true" />
                            <span className="text-[#3fb950]">已复制</span>
                        </>
                    ) : (
                        <>
                            <Copy className="h-3 w-3" aria-hidden="true" />
                            <span>复制</span>
                        </>
                    )}
                </button>
            </div>

            <pre className="silva-code-block-pre">
                <NodeViewContent<'code'>
                    as="code"
                    className={rawLanguage ? `language-${rawLanguage}` : undefined}
                />
            </pre>
        </NodeViewWrapper>
    );
}

/**
 * 扩展自 @tiptap/extension-code-block：
 * - 完整保留 Markdown fenced code block 语言解析与序列化（```lang）
 * - 提供内联语言选择器与复制按钮
 * - 通过 ProseMirror Decoration 接入按需加载的 Shiki 语法高亮
 */
export const SilvaCodeBlock = CodeBlock.extend({
    addNodeView() {
        return ReactNodeViewRenderer(SilvaCodeBlockNodeView, {
            stopEvent({ event }) {
                const target = event.target as HTMLElement | null;
                return Boolean(target?.closest('[data-silva-code-controls]'));
            },
        });
    },
    addProseMirrorPlugins() {
        return [...(this.parent?.() ?? []), createShikiHighlightPlugin()];
    },
});
