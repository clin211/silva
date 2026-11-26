import { AlertTriangle, CheckCircle2, Info, X, XCircle } from 'lucide-react';
import { createContext, use, useCallback, useMemo, useState, type ReactNode } from 'react';
import { cn } from '@/lib/cn';

export type ToastTone = 'info' | 'ok' | 'warn' | 'bad';

export interface ToastItem {
    id: string;
    message: string;
    tone: ToastTone;
    actionLabel?: string;
    onAction?: () => void;
}

interface ToastContextValue {
    toast: (
        message: string,
        tone?: ToastTone,
        action?: { label: string; onClick: () => void },
    ) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

const TONE_STYLES: Record<ToastTone, string> = {
    info: 'border-ui-line bg-white text-ui-text',
    ok: 'border-ok/25 bg-white text-ui-text',
    warn: 'border-warn/30 bg-white text-ui-text',
    bad: 'border-bad/30 bg-white text-ui-text',
};

function ToastIcon({ tone }: { tone: ToastTone }) {
    switch (tone) {
        case 'ok':
            return <CheckCircle2 className="h-4 w-4 shrink-0 text-ok" aria-hidden="true" />;
        case 'warn':
            return <AlertTriangle className="h-4 w-4 shrink-0 text-warn" aria-hidden="true" />;
        case 'bad':
            return <XCircle className="h-4 w-4 shrink-0 text-bad" aria-hidden="true" />;
        default:
            return <Info className="h-4 w-4 shrink-0 text-accent" aria-hidden="true" />;
    }
}

export function ToastProvider({ children }: { children: ReactNode }) {
    const [items, setItems] = useState<ToastItem[]>([]);

    const dismiss = useCallback((id: string) => {
        setItems((prev) => prev.filter((item) => item.id !== id));
    }, []);

    const toast = useCallback(
        (
            message: string,
            tone: ToastTone = 'info',
            action?: { label: string; onClick: () => void },
        ) => {
            const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
            setItems((prev) => [
                ...prev.slice(-3),
                {
                    id,
                    message,
                    tone,
                    actionLabel: action?.label,
                    onAction: action?.onClick,
                },
            ]);
            window.setTimeout(() => {
                dismiss(id);
            }, 4800);
        },
        [dismiss],
    );

    const value = useMemo(() => ({ toast }), [toast]);

    return (
        <ToastContext value={value}>
            {children}
            <div
                aria-live="polite"
                aria-atomic="false"
                className="pointer-events-none fixed right-5 bottom-5 z-[80] flex max-w-md flex-col gap-2"
            >
                {items.map((item) => (
                    <div
                        key={item.id}
                        role={item.tone === 'bad' ? 'alert' : 'status'}
                        className={cn(
                            'animate-modal-in pointer-events-auto flex items-center gap-2.5 rounded-xl border px-4 py-3 text-[13.5px] shadow-[var(--shadow-overlay)]',
                            TONE_STYLES[item.tone],
                        )}
                    >
                        <ToastIcon tone={item.tone} />
                        <span className="flex-1 leading-snug">{item.message}</span>
                        {item.actionLabel && item.onAction ? (
                            <button
                                type="button"
                                onClick={() => {
                                    item.onAction?.();
                                    dismiss(item.id);
                                }}
                                className="shrink-0 font-medium text-accent underline underline-offset-2"
                            >
                                {item.actionLabel}
                            </button>
                        ) : null}
                        <button
                            type="button"
                            onClick={() => dismiss(item.id)}
                            className="shrink-0 rounded-md p-1 text-ui-sub hover:bg-ui-bg hover:text-ui-text"
                            aria-label="关闭通知"
                        >
                            <X className="h-3.5 w-3.5" />
                        </button>
                    </div>
                ))}
            </div>
        </ToastContext>
    );
}

export function useToast(): ToastContextValue {
    const ctx = use(ToastContext);
    if (!ctx) {
        throw new Error('useToast must be used within ToastProvider');
    }
    return ctx;
}
