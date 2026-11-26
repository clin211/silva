import { Extension } from '@tiptap/core';
import Image from '@tiptap/extension-image';
import { Placeholder } from '@tiptap/extensions';
import { Markdown } from '@tiptap/markdown';
import { Plugin, PluginKey, type EditorState } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import {
    EditorContent,
    NodeViewWrapper,
    ReactNodeViewRenderer,
    useEditor,
    useEditorState,
    type NodeViewProps,
} from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import {
    Bold,
    Code,
    FileCode2,
    Heading2,
    Heading3,
    ImagePlus,
    Italic,
    List,
    ListOrdered,
    Minus,
    Quote,
    Redo2,
    Strikethrough,
    Undo2,
} from 'lucide-react';
import {
    useEffect,
    useId,
    useImperativeHandle,
    useRef,
    useState,
    type MouseEvent as ReactMouseEvent,
    type ReactNode,
    type Ref,
} from 'react';
import { AuthenticatedImage } from '@/components/media/authenticated-media';
import { cn } from '@/lib/cn';
import {
    DEFAULT_IMAGE_ALT_COLOR,
    formatImageAltColorTitle,
    IMAGE_ALT_COLOR_PRESETS,
    parseImageAltColor,
} from '@/lib/markdown';

const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

type UploadPlaceholderAction =
    | { add: { id: string; pos: number; label: string } }
    | { remove: { id: string } };

const UPLOAD_PLACEHOLDER_PLUGIN_KEY = new PluginKey<DecorationSet>('silvaUploadPlaceholder');

function createUploadPlaceholderWidget(label: string): HTMLElement {
    const badge = document.createElement('span');
    badge.className = 'tiptap-upload-placeholder';
    badge.setAttribute('role', 'status');
    badge.setAttribute('contenteditable', 'false');

    const spinner = document.createElement('span');
    spinner.className = 'tiptap-upload-spinner';
    spinner.setAttribute('aria-hidden', 'true');

    const text = document.createElement('span');
    text.textContent = label;

    badge.append(spinner, text);
    return badge;
}

const UploadPlaceholderExtension = Extension.create({
    name: 'silvaUploadPlaceholder',
    addProseMirrorPlugins() {
        return [
            new Plugin<DecorationSet>({
                key: UPLOAD_PLACEHOLDER_PLUGIN_KEY,
                state: {
                    init() {
                        return DecorationSet.empty;
                    },
                    apply(tr, set) {
                        let nextSet = set.map(tr.mapping, tr.doc);
                        const action = tr.getMeta(UPLOAD_PLACEHOLDER_PLUGIN_KEY) as
                            | UploadPlaceholderAction
                            | undefined;
                        if (!action) return nextSet;
                        if ('add' in action) {
                            const { id, pos, label } = action.add;
                            const clamped = Math.min(Math.max(0, pos), tr.doc.content.size);
                            const widget = Decoration.widget(
                                clamped,
                                () => createUploadPlaceholderWidget(label),
                                { id },
                            );
                            nextSet = nextSet.add(tr.doc, [widget]);
                        } else if ('remove' in action) {
                            const found = nextSet.find(
                                undefined,
                                undefined,
                                (spec: { id?: string }) => spec.id === action.remove.id,
                            );
                            if (found.length > 0) {
                                nextSet = nextSet.remove(found);
                            }
                        }
                        return nextSet;
                    },
                },
                props: {
                    decorations(state) {
                        return this.getState(state);
                    },
                },
            }),
        ];
    },
});

function findUploadPlaceholderPos(state: EditorState, id: string): number | null {
    const set = UPLOAD_PLACEHOLDER_PLUGIN_KEY.getState(state);
    if (!set) return null;
    const found = set.find(undefined, undefined, (spec: { id?: string }) => spec.id === id);
    return found[0]?.from ?? null;
}

function extractClipboardImageFiles(data: DataTransfer | null | undefined): File[] {
    if (!data) return [];
    const fromItems: File[] = [];
    if (data.items) {
        for (const item of Array.from(data.items)) {
            if (item.kind === 'file' && item.type.startsWith('image/')) {
                const file = item.getAsFile();
                if (file) fromItems.push(file);
            }
        }
    }
    if (fromItems.length > 0) return fromItems;
    if (data.files) {
        return Array.from(data.files).filter((file) => file.type.startsWith('image/'));
    }
    return [];
}

function extractMediaId(src: string): string | null {
    const trimmed = src.trim();
    if (!trimmed.startsWith('media:')) return null;
    const candidate = trimmed.slice('media:'.length).trim();
    return UUID_REGEX.test(candidate) ? candidate : null;
}

function ImageAltPopover({
    initialAlt,
    initialColor,
    onPreviewChange,
    onCommit,
    onDismiss,
}: {
    initialAlt: string;
    initialColor: string;
    onPreviewChange: (preview: { alt: string; color: string } | null) => void;
    onCommit: (nextAlt: string, nextColor: string) => void;
    onDismiss: () => void;
}) {
    const altInputId = useId();
    const [altDraft, setAltDraft] = useState(initialAlt);
    const [colorDraft, setColorDraft] = useState(initialColor);

    const submit = () => {
        const normalized = altDraft.trim();
        onPreviewChange(null);
        if (normalized !== initialAlt || colorDraft !== initialColor) {
            onCommit(normalized, colorDraft);
        }
    };

    const handleAltChange = (nextAlt: string) => {
        setAltDraft(nextAlt);
        onPreviewChange({ alt: nextAlt, color: colorDraft });
    };

    const handleColorChange = (nextColor: string) => {
        const normalized = nextColor.toLowerCase();
        setColorDraft(normalized);
        onPreviewChange({ alt: altDraft, color: normalized });
    };

    return (
        <span
            data-silva-image-controls=""
            contentEditable={false}
            className="absolute top-full left-0 z-20 mt-2 flex flex-wrap items-center gap-2 rounded-lg border border-ui-line bg-white px-2.5 py-2 shadow-[var(--shadow-overlay)]"
        >
            <span className="flex items-center gap-1.5">
                <label
                    htmlFor={altInputId}
                    className="shrink-0 text-[11.5px] font-medium text-ui-sub"
                >
                    Alt
                </label>
                <input
                    id={altInputId}
                    type="text"
                    value={altDraft}
                    placeholder="输入图片 Alt 说明（可选）"
                    onChange={(event) => handleAltChange(event.target.value)}
                    onKeyDown={(event) => {
                        if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
                            event.preventDefault();
                            submit();
                            onDismiss();
                        } else if (event.key === 'Escape') {
                            event.preventDefault();
                            setAltDraft(initialAlt);
                            setColorDraft(initialColor);
                            onPreviewChange(null);
                            onDismiss();
                        }
                    }}
                    className="h-7 w-48 rounded-md border border-ui-line bg-ui-subtle px-2 text-[12.5px] text-ui-text placeholder:text-[#98ABA8] focus:border-accent focus:bg-white focus:outline-none"
                />
            </span>

            <span className="h-4 w-px shrink-0 bg-ui-line" aria-hidden="true" />

            <span className="flex items-center gap-1" aria-label="Alt 字体颜色">
                {IMAGE_ALT_COLOR_PRESETS.map((preset) => {
                    const active = colorDraft.toLowerCase() === preset.value;
                    return (
                        <button
                            key={preset.value}
                            type="button"
                            title={preset.label}
                            aria-label={`颜色：${preset.label}`}
                            aria-pressed={active}
                            onMouseDown={(event) => event.preventDefault()}
                            onClick={() => handleColorChange(preset.value)}
                            className={cn(
                                'grid h-5 w-5 place-items-center rounded-full border transition-transform hover:scale-110',
                                active ? 'border-accent ring-2 ring-accent/30' : 'border-black/15',
                            )}
                            style={{ backgroundColor: preset.value }}
                        />
                    );
                })}
                <label
                    title="自定义颜色"
                    className="relative ml-0.5 grid h-5 w-5 cursor-pointer place-items-center overflow-hidden rounded-full border border-ui-line-strong bg-[conic-gradient(#ef4444,#f59e0b,#10b981,#3b82f6,#8b5cf6,#ef4444)] transition-transform hover:scale-110"
                >
                    <input
                        type="color"
                        aria-label="自定义 Alt 颜色"
                        value={colorDraft}
                        onChange={(event) => handleColorChange(event.target.value)}
                        className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
                    />
                </label>
            </span>

            <button
                type="button"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                    submit();
                    onDismiss();
                }}
                className="h-7 shrink-0 rounded-md bg-accent px-2.5 text-[12px] font-medium text-white transition-colors hover:bg-accent-2"
            >
                确定
            </button>
        </span>
    );
}

function SilvaImageNodeView({ node, selected, updateAttributes, editor, getPos }: NodeViewProps) {
    const src = typeof node.attrs.src === 'string' ? node.attrs.src : '';
    const alt = typeof node.attrs.alt === 'string' ? node.attrs.alt : '';
    const rawTitle = typeof node.attrs.title === 'string' ? node.attrs.title : undefined;
    const altColor = parseImageAltColor(rawTitle);
    const effectiveAltColor = altColor ?? DEFAULT_IMAGE_ALT_COLOR;
    const visibleTitle = altColor ? undefined : rawTitle;
    const mediaId = extractMediaId(src);
    const [livePreview, setLivePreview] = useState<{ alt: string; color: string } | null>(null);

    const displayedAlt = selected && livePreview ? livePreview.alt : alt;
    const displayedColor = selected && livePreview ? livePreview.color : effectiveAltColor;

    const selectImageNode = (event: ReactMouseEvent<HTMLElement>) => {
        if (!editor.isEditable) return;
        if ((event.target as HTMLElement | null)?.closest('[data-silva-image-controls]')) {
            return;
        }
        const pos = getPos();
        if (typeof pos === 'number') {
            editor.commands.setNodeSelection(pos);
        }
    };

    return (
        <NodeViewWrapper
            as="span"
            onClick={selectImageNode}
            className={cn(
                'relative my-1 inline-block max-w-full rounded-lg align-middle transition-shadow',
                selected && 'ring-2 ring-accent/50 ring-offset-2',
            )}
            data-drag-handle=""
        >
            {mediaId ? (
                <AuthenticatedImage
                    mediaId={mediaId}
                    alt={alt}
                    className="block max-h-[460px] w-auto max-w-full rounded-lg border border-ui-line object-contain"
                    fallbackClassName="inline-grid h-44 w-72 max-w-full rounded-lg border border-dashed border-ui-line"
                />
            ) : /^https?:\/\//i.test(src.trim()) || src.trim().startsWith('/') ? (
                <img
                    src={src}
                    alt={alt}
                    title={visibleTitle}
                    className="block max-h-[460px] w-auto max-w-full rounded-lg border border-ui-line object-contain"
                />
            ) : (
                <span className="inline-block rounded-lg border border-dashed border-ui-line bg-ui-subtle px-3.5 py-2.5 font-code text-[12.5px] text-ui-sub">
                    [图片引用：{src || '未设置地址'}]
                </span>
            )}

            {displayedAlt.trim() ? (
                <span
                    className="mt-1.5 block text-center text-[12.5px] leading-snug"
                    style={{ color: displayedColor }}
                >
                    {displayedAlt}
                </span>
            ) : null}

            {selected && editor.isEditable ? (
                <ImageAltPopover
                    key={`${alt}:${effectiveAltColor}`}
                    initialAlt={alt}
                    initialColor={effectiveAltColor}
                    onPreviewChange={setLivePreview}
                    onCommit={(nextAlt, nextColor) =>
                        updateAttributes({
                            alt: nextAlt,
                            title: formatImageAltColorTitle(nextColor),
                        })
                    }
                    onDismiss={() => {
                        setLivePreview(null);
                        const pos = getPos();
                        if (typeof pos === 'number') {
                            editor
                                .chain()
                                .setTextSelection(pos + node.nodeSize)
                                .focus()
                                .run();
                        } else {
                            editor.commands.focus();
                        }
                    }}
                />
            ) : null}
        </NodeViewWrapper>
    );
}

const SilvaImage = Image.extend({
    addNodeView() {
        return ReactNodeViewRenderer(SilvaImageNodeView, {
            stopEvent({ event }) {
                const target = event.target as HTMLElement | null;
                return Boolean(target?.closest('[data-silva-image-controls]'));
            },
        });
    },
});

export interface UploadedEditorImage {
    src: string;
    alt?: string;
}

export interface TiptapMarkdownEditorHandle {
    insertImage: (options: UploadedEditorImage) => void;
    focus: () => void;
}

export interface TiptapMarkdownEditorProps {
    value: string;
    onChange: (nextValue: string) => void;
    onRequestInsertMedia?: () => void;
    onUploadImage?: (file: File) => Promise<UploadedEditorImage | null>;
    placeholder?: string;
    readOnly?: boolean;
    ref?: Ref<TiptapMarkdownEditorHandle>;
}

function ToolbarButton({
    active = false,
    disabled = false,
    title,
    onClick,
    children,
}: {
    active?: boolean;
    disabled?: boolean;
    title: string;
    onClick: () => void;
    children: ReactNode;
}) {
    return (
        <button
            type="button"
            title={title}
            aria-label={title}
            aria-pressed={active}
            disabled={disabled}
            onMouseDown={(event) => event.preventDefault()}
            onClick={onClick}
            className={cn(
                'rounded-md p-1.5 transition-colors disabled:pointer-events-none disabled:opacity-40',
                active
                    ? 'bg-accent-tint text-accent'
                    : 'text-ui-text hover:bg-[#E7EFED] hover:text-ui-text',
            )}
        >
            {children}
        </button>
    );
}

/**
 * 基于 Tiptap 与 @tiptap/markdown 的富文本 Markdown 双向编辑器。
 * 支持所见即所得写作、Markdown 快捷输入、粘贴/拖拽图片自动上传到光标位置、
 * media:<ID> 认证图片预览与无损 Markdown 序列化。
 */
export function TiptapMarkdownEditor({
    value,
    onChange,
    onRequestInsertMedia,
    onUploadImage,
    placeholder = '开始写作，支持 Markdown 快捷语法与直接粘贴图片……',
    readOnly = false,
    ref,
}: TiptapMarkdownEditorProps) {
    const onChangeRef = useRef(onChange);
    const onUploadImageRef = useRef(onUploadImage);
    const readOnlyRef = useRef(readOnly);
    const lastSyncedMarkdownRef = useRef(value);
    const uploadSeqRef = useRef(0);

    useEffect(() => {
        onChangeRef.current = onChange;
    }, [onChange]);

    useEffect(() => {
        onUploadImageRef.current = onUploadImage;
    }, [onUploadImage]);

    useEffect(() => {
        readOnlyRef.current = readOnly;
    }, [readOnly]);

    const uploadImagesAtPosRef = useRef<((files: File[], initialPos: number) => void) | undefined>(
        undefined,
    );

    const editor = useEditor({
        extensions: [
            StarterKit.configure({
                heading: { levels: [1, 2, 3, 4] },
                link: {
                    openOnClick: false,
                    autolink: true,
                },
                underline: false,
            }),
            SilvaImage.configure({
                inline: true,
                allowBase64: false,
            }),
            Placeholder.configure({
                placeholder,
            }),
            Markdown.configure({
                markedOptions: {
                    gfm: true,
                    breaks: false,
                },
            }),
            UploadPlaceholderExtension,
        ],
        content: value,
        contentType: 'markdown',
        editable: !readOnly,
        editorProps: {
            attributes: {
                'aria-label': 'Markdown 正文编辑器',
                class: 'tiptap-silva-content prose-ink min-h-full px-5 py-5 focus:outline-none sm:px-6',
            },
            handlePaste(view, event) {
                if (readOnlyRef.current || !onUploadImageRef.current) return false;
                const imageFiles = extractClipboardImageFiles(event.clipboardData);
                if (imageFiles.length === 0) return false;
                event.preventDefault();
                const insertPos = view.state.selection.to;
                uploadImagesAtPosRef.current?.(imageFiles, insertPos);
                return true;
            },
            handleDrop(view, event, _slice, moved) {
                if (moved || readOnlyRef.current || !onUploadImageRef.current) return false;
                const imageFiles = extractClipboardImageFiles(event.dataTransfer);
                if (imageFiles.length === 0) return false;
                event.preventDefault();
                const coords = view.posAtCoords({
                    left: event.clientX,
                    top: event.clientY,
                });
                const insertPos = coords?.pos ?? view.state.selection.to;
                uploadImagesAtPosRef.current?.(imageFiles, insertPos);
                return true;
            },
        },
        onUpdate({ editor: activeEditor }) {
            const markdown = activeEditor.getMarkdown();
            if (markdown === lastSyncedMarkdownRef.current) return;
            lastSyncedMarkdownRef.current = markdown;
            onChangeRef.current(markdown);
        },
    });

    // 绑定异步图片上传并按占位符映射后的精准位置插入节点
    useEffect(() => {
        uploadImagesAtPosRef.current = (files: File[], initialPos: number) => {
            if (!editor || editor.isDestroyed) return;
            const uploader = onUploadImageRef.current;
            if (!uploader) return;

            const entries = files.map((file) => {
                uploadSeqRef.current += 1;
                const id = `upload-${uploadSeqRef.current}`;
                const label = file.name ? `上传中：${file.name}` : '正在上传图片…';
                return { id, file, label };
            });

            // 先在同一事务中为全部待上传图片放置占位符，保证并发/后续编辑时位置准确跟踪
            let addTr = editor.state.tr;
            for (const entry of entries) {
                addTr = addTr.setMeta(UPLOAD_PLACEHOLDER_PLUGIN_KEY, {
                    add: { id: entry.id, pos: initialPos, label: entry.label },
                } satisfies UploadPlaceholderAction);
                editor.view.dispatch(addTr);
                addTr = editor.state.tr;
            }

            void (async () => {
                for (const entry of entries) {
                    let uploaded: UploadedEditorImage | null = null;
                    try {
                        uploaded = await uploader(entry.file);
                    } catch {
                        uploaded = null;
                    }

                    if (editor.isDestroyed) return;
                    const mappedPos = findUploadPlaceholderPos(editor.state, entry.id);

                    // 移除当前占位符
                    editor.view.dispatch(
                        editor.state.tr.setMeta(UPLOAD_PLACEHOLDER_PLUGIN_KEY, {
                            remove: { id: entry.id },
                        } satisfies UploadPlaceholderAction),
                    );

                    if (!uploaded) continue;
                    const safePos = Math.min(
                        Math.max(0, mappedPos ?? editor.state.selection.to),
                        editor.state.doc.content.size,
                    );
                    editor
                        .chain()
                        .insertContentAt(safePos, {
                            type: 'image',
                            attrs: {
                                src: uploaded.src,
                                alt: uploaded.alt ?? '',
                            },
                        })
                        .run();
                }
            })();
        };
    }, [editor]);

    // 同步只读状态
    useEffect(() => {
        if (!editor) return;
        editor.setEditable(!readOnly);
    }, [editor, readOnly]);

    // 外部重新加载文章或冲突后读取最新修订时，同步 Markdown 到编辑器
    useEffect(() => {
        if (!editor) return;
        if (value === lastSyncedMarkdownRef.current) return;
        lastSyncedMarkdownRef.current = value;
        editor.commands.setContent(value, {
            contentType: 'markdown',
            emitUpdate: false,
        });
    }, [editor, value]);

    useImperativeHandle(
        ref,
        () => ({
            insertImage({ src, alt }) {
                if (!editor) return;
                editor
                    .chain()
                    .focus()
                    .setImage({ src, alt: alt ?? '' })
                    .run();
            },
            focus() {
                editor?.commands.focus();
            },
        }),
        [editor],
    );

    const toolbarState = useEditorState({
        editor,
        selector: ({ editor: activeEditor }) => {
            if (!activeEditor) {
                return {
                    bold: false,
                    italic: false,
                    strike: false,
                    code: false,
                    h2: false,
                    h3: false,
                    bulletList: false,
                    orderedList: false,
                    blockquote: false,
                    codeBlock: false,
                    canUndo: false,
                    canRedo: false,
                };
            }
            return {
                bold: activeEditor.isActive('bold'),
                italic: activeEditor.isActive('italic'),
                strike: activeEditor.isActive('strike'),
                code: activeEditor.isActive('code'),
                h2: activeEditor.isActive('heading', { level: 2 }),
                h3: activeEditor.isActive('heading', { level: 3 }),
                bulletList: activeEditor.isActive('bulletList'),
                orderedList: activeEditor.isActive('orderedList'),
                blockquote: activeEditor.isActive('blockquote'),
                codeBlock: activeEditor.isActive('codeBlock'),
                canUndo: activeEditor.can().undo(),
                canRedo: activeEditor.can().redo(),
            };
        },
    });

    return (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            {/* 格式工具栏 */}
            <div className="flex min-h-10 shrink-0 flex-wrap items-center justify-between gap-2 border-b border-ui-line bg-ui-subtle/70 px-3 py-1.5 text-[13px] sm:px-4">
                <div className="flex flex-wrap items-center gap-0.5">
                    <ToolbarButton
                        title="撤销"
                        disabled={readOnly || !toolbarState?.canUndo}
                        onClick={() => editor?.chain().focus().undo().run()}
                    >
                        <Undo2 className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="重做"
                        disabled={readOnly || !toolbarState?.canRedo}
                        onClick={() => editor?.chain().focus().redo().run()}
                    >
                        <Redo2 className="h-3.5 w-3.5" />
                    </ToolbarButton>

                    <span className="mx-1 h-4 w-px bg-ui-line" aria-hidden="true" />

                    <ToolbarButton
                        title="二级标题"
                        active={Boolean(toolbarState?.h2)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleHeading({ level: 2 }).run()}
                    >
                        <Heading2 className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="三级标题"
                        active={Boolean(toolbarState?.h3)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleHeading({ level: 3 }).run()}
                    >
                        <Heading3 className="h-3.5 w-3.5" />
                    </ToolbarButton>

                    <span className="mx-1 h-4 w-px bg-ui-line" aria-hidden="true" />

                    <ToolbarButton
                        title="粗体"
                        active={Boolean(toolbarState?.bold)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleBold().run()}
                    >
                        <Bold className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="斜体"
                        active={Boolean(toolbarState?.italic)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleItalic().run()}
                    >
                        <Italic className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="删除线"
                        active={Boolean(toolbarState?.strike)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleStrike().run()}
                    >
                        <Strikethrough className="h-3.5 w-3.5" />
                    </ToolbarButton>

                    <span className="mx-1 h-4 w-px bg-ui-line" aria-hidden="true" />

                    <ToolbarButton
                        title="无序列表"
                        active={Boolean(toolbarState?.bulletList)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleBulletList().run()}
                    >
                        <List className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="有序列表"
                        active={Boolean(toolbarState?.orderedList)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleOrderedList().run()}
                    >
                        <ListOrdered className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="引用"
                        active={Boolean(toolbarState?.blockquote)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleBlockquote().run()}
                    >
                        <Quote className="h-3.5 w-3.5" />
                    </ToolbarButton>

                    <span className="mx-1 h-4 w-px bg-ui-line" aria-hidden="true" />

                    <ToolbarButton
                        title="行内代码"
                        active={Boolean(toolbarState?.code)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleCode().run()}
                    >
                        <Code className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="代码块"
                        active={Boolean(toolbarState?.codeBlock)}
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().toggleCodeBlock().run()}
                    >
                        <FileCode2 className="h-3.5 w-3.5" />
                    </ToolbarButton>
                    <ToolbarButton
                        title="分隔线"
                        disabled={readOnly}
                        onClick={() => editor?.chain().focus().setHorizontalRule().run()}
                    >
                        <Minus className="h-3.5 w-3.5" />
                    </ToolbarButton>

                    {onRequestInsertMedia ? (
                        <>
                            <span className="mx-1 h-4 w-px bg-ui-line" aria-hidden="true" />
                            <button
                                type="button"
                                disabled={readOnly}
                                onMouseDown={(event) => event.preventDefault()}
                                onClick={onRequestInsertMedia}
                                className="inline-flex h-7 shrink-0 items-center gap-1 rounded-md px-2 text-[12px] font-medium whitespace-nowrap text-accent transition-colors hover:bg-accent-tint disabled:pointer-events-none disabled:opacity-40"
                            >
                                <ImagePlus className="h-3.5 w-3.5 shrink-0" />
                                插入媒体
                            </button>
                        </>
                    ) : null}
                </div>
                <span className="hidden font-code text-[11px] font-medium tracking-[0.04em] text-ui-sub/80 sm:inline">
                    MARKDOWN
                </span>
            </div>

            {/* Tiptap 编辑画布 */}
            <EditorContent
                editor={editor}
                className="tiptap-silva-host min-h-0 flex-1 overflow-y-auto bg-white"
            />
        </div>
    );
}
