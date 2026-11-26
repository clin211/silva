import { useEffect, useId, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';
import { cn } from '@/lib/cn';

export interface ModalProps {
    open: boolean;
    onClose: () => void;
    title: string;
    description?: string;
    role?: 'dialog' | 'alertdialog';
    maxWidthClassName?: string;
    children: ReactNode;
    footer?: ReactNode;
}

export function Modal({
    open,
    onClose,
    title,
    description,
    role = 'dialog',
    maxWidthClassName = 'max-w-[480px]',
    children,
    footer,
}: ModalProps) {
    const titleId = useId();
    const descId = useId();
    const containerRef = useRef<HTMLDivElement>(null);
    const previousFocusRef = useRef<HTMLElement | null>(null);
    const onCloseRef = useRef(onClose);

    useEffect(() => {
        onCloseRef.current = onClose;
    }, [onClose]);

    useEffect(() => {
        if (!open) return;
        previousFocusRef.current = document.activeElement as HTMLElement | null;
        const focusTarget = containerRef.current?.querySelector<HTMLElement>(
            'input, textarea, select, button:not([data-modal-close])',
        );
        focusTarget?.focus();

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.preventDefault();
                onCloseRef.current();
            }
        };
        document.addEventListener('keydown', handleKeyDown);
        return () => {
            document.removeEventListener('keydown', handleKeyDown);
            previousFocusRef.current?.focus();
        };
    }, [open]);

    if (!open) return null;

    return (
        <div className="fixed inset-0 z-50 grid place-items-center p-4">
            <div
                aria-hidden="true"
                onClick={onClose}
                className="animate-fade-in fixed inset-0 bg-[#091C1A]/45 backdrop-blur-[2px]"
            />
            <div
                ref={containerRef}
                role={role}
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={description ? descId : undefined}
                className={cn(
                    'animate-modal-in relative z-10 w-full overflow-hidden rounded-2xl border border-ui-line bg-white shadow-[var(--shadow-overlay)]',
                    maxWidthClassName,
                )}
            >
                <div className="flex items-start justify-between gap-3 px-6 pt-5 pb-1">
                    <div className="min-w-0">
                        <h2
                            id={titleId}
                            className="text-[17px] font-semibold tracking-[-0.01em] text-ui-text"
                        >
                            {title}
                        </h2>
                        {description ? (
                            <p id={descId} className="mt-1.5 text-[13.5px] leading-6 text-ui-sub">
                                {description}
                            </p>
                        ) : null}
                    </div>
                    <button
                        type="button"
                        data-modal-close=""
                        onClick={onClose}
                        aria-label="关闭弹窗"
                        className="-mr-1 rounded-lg p-1.5 text-ui-sub transition-colors hover:bg-ui-bg hover:text-ui-text"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </div>
                <div className="px-6 py-4">{children}</div>
                {footer ? (
                    <div className="flex justify-end gap-2 border-t border-ui-line bg-ui-subtle px-6 py-3.5">
                        {footer}
                    </div>
                ) : null}
            </div>
        </div>
    );
}

export interface DrawerProps {
    open: boolean;
    onClose: () => void;
    title: string;
    subtitle?: string;
    widthClassName?: string;
    children: ReactNode;
    footer?: ReactNode;
}

export function Drawer({
    open,
    onClose,
    title,
    subtitle,
    widthClassName = 'sm:w-[460px]',
    children,
    footer,
}: DrawerProps) {
    const titleId = useId();
    const panelRef = useRef<HTMLDialogElement>(null);
    const previousFocusRef = useRef<HTMLElement | null>(null);
    const onCloseRef = useRef(onClose);

    useEffect(() => {
        onCloseRef.current = onClose;
    }, [onClose]);

    useEffect(() => {
        if (!open) return;
        previousFocusRef.current = document.activeElement as HTMLElement | null;
        const focusTarget = panelRef.current?.querySelector<HTMLElement>(
            'input, textarea, select, button:not([data-drawer-close])',
        );
        focusTarget?.focus();

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.preventDefault();
                onCloseRef.current();
            }
        };
        document.addEventListener('keydown', handleKeyDown);
        return () => {
            document.removeEventListener('keydown', handleKeyDown);
            previousFocusRef.current?.focus();
        };
    }, [open]);

    if (!open) return null;

    return (
        <div className="fixed inset-0 z-50">
            <div
                aria-hidden="true"
                onClick={onClose}
                className="animate-fade-in fixed inset-0 bg-[#091C1A]/45 backdrop-blur-[2px]"
            />
            <dialog
                ref={panelRef}
                open
                aria-modal="true"
                aria-labelledby={titleId}
                className={cn(
                    'animate-drawer-right fixed top-0 right-0 left-auto z-10 m-0 flex h-full w-full flex-col border-0 border-l border-ui-line bg-white p-0 text-ui-text shadow-[var(--shadow-overlay)]',
                    widthClassName,
                )}
            >
                <header className="flex h-16 shrink-0 items-center justify-between gap-3 border-b border-ui-line px-6">
                    <div className="min-w-0">
                        <h2
                            id={titleId}
                            className="truncate text-[16px] font-semibold tracking-[-0.01em] text-ui-text"
                        >
                            {title}
                        </h2>
                        {subtitle ? (
                            <p className="mt-0.5 truncate font-code text-[12px] text-ui-sub">
                                {subtitle}
                            </p>
                        ) : null}
                    </div>
                    <button
                        type="button"
                        data-drawer-close=""
                        onClick={onClose}
                        aria-label="关闭抽屉"
                        className="rounded-lg p-1.5 text-ui-sub transition-colors hover:bg-ui-bg hover:text-ui-text"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </header>
                <div className="flex-1 overflow-y-auto px-6 py-5">{children}</div>
                {footer ? (
                    <footer className="shrink-0 border-t border-ui-line bg-ui-subtle px-6 py-3.5">
                        {footer}
                    </footer>
                ) : null}
            </dialog>
        </div>
    );
}
