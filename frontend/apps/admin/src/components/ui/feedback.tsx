import { AlertCircle, ChevronLeft, ChevronRight, Inbox } from 'lucide-react';
import type { ReactNode } from 'react';
import { Button } from './button';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId } from '@/lib/errors';

export function PageContainer({
    children,
    size = 'default',
    className,
}: {
    children: ReactNode;
    size?: 'default' | 'narrow' | 'fluid';
    className?: string;
}) {
    return (
        <div
            className={cn(
                'mx-auto w-full',
                size === 'default' && 'max-w-[1320px]',
                size === 'narrow' && 'max-w-[980px]',
                size === 'fluid' && 'max-w-none',
                className,
            )}
        >
            {children}
        </div>
    );
}

export function PageHeader({
    eyebrow,
    title,
    description,
    actions,
    badge,
}: {
    eyebrow?: string;
    title: string;
    description?: string;
    actions?: ReactNode;
    badge?: ReactNode;
}) {
    return (
        <div className="flex flex-wrap items-end justify-between gap-4">
            <div className="min-w-0">
                {eyebrow ? (
                    <p className="mb-1 text-[11.5px] font-medium tracking-[0.06em] text-accent uppercase">
                        {eyebrow}
                    </p>
                ) : null}
                <div className="flex flex-wrap items-center gap-2.5">
                    <h1 className="text-[24px] leading-tight font-semibold tracking-[-0.02em] text-ui-text">
                        {title}
                    </h1>
                    {badge}
                </div>
                {description ? (
                    <p className="mt-1.5 max-w-[740px] text-[13.5px] leading-relaxed text-ui-sub">
                        {description}
                    </p>
                ) : null}
            </div>
            {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
        </div>
    );
}

export function ErrorBanner({
    title = '加载失败',
    error,
    onRetry,
    className,
}: {
    title?: string;
    error: unknown;
    onRetry?: () => void;
    className?: string;
}) {
    return (
        <div
            role="alert"
            className={cn(
                'flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#F0C7C2] bg-bad-tint px-4 py-3.5 text-[13.5px] text-bad',
                className,
            )}
        >
            <div className="flex items-start gap-2.5">
                <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                <div>
                    <strong className="font-medium">{title}：</strong>
                    <span>{formatErrorWithRequestId(error)}</span>
                </div>
            </div>
            {onRetry ? (
                <Button variant="secondary" size="sm" onClick={onRetry}>
                    重试
                </Button>
            ) : null}
        </div>
    );
}

export function EmptyState({
    title,
    description,
    action,
    icon,
    className,
}: {
    title: string;
    description?: string;
    action?: ReactNode;
    icon?: ReactNode;
    className?: string;
}) {
    return (
        <div
            className={cn(
                'studio-card flex flex-col items-center justify-center px-6 py-14 text-center',
                className,
            )}
        >
            <div className="grid h-12 w-12 place-items-center rounded-2xl bg-accent-tint text-accent">
                {icon ?? <Inbox className="h-5 w-5" aria-hidden="true" />}
            </div>
            <p className="mt-4 text-[15px] font-semibold text-ui-text">{title}</p>
            {description ? (
                <p className="mt-1 max-w-md text-[13.5px] leading-relaxed text-ui-sub">
                    {description}
                </p>
            ) : null}
            {action ? (
                <div className="mt-5 flex flex-wrap justify-center gap-2">{action}</div>
            ) : null}
        </div>
    );
}

export function LoadingCard({ text = '正在加载…' }: { text?: string }) {
    return (
        <div className="studio-card p-5" aria-busy="true" aria-label={text}>
            <div className="space-y-3">
                <div className="h-4 w-44 animate-pulse rounded-md bg-[#E7EEEC]" />
                <div className="h-10 w-full animate-pulse rounded-lg bg-[#EFF4F2]" />
                <div className="h-10 w-full animate-pulse rounded-lg bg-[#EFF4F2]" />
                <div className="h-10 w-5/6 animate-pulse rounded-lg bg-[#EFF4F2]" />
            </div>
            <p className="mt-4 text-[12.5px] text-ui-sub">{text}</p>
        </div>
    );
}

export interface PaginationBarProps {
    page: number;
    perPage: number;
    total: number;
    totalPages: number;
    onPageChange: (page: number) => void;
    unit?: string;
    perPageOptions?: number[];
    onPerPageChange?: (perPage: number) => void;
}

export function PaginationBar({
    page,
    perPage,
    total,
    totalPages,
    onPageChange,
    unit = '条',
    perPageOptions,
    onPerPageChange,
}: PaginationBarProps) {
    const safeTotalPages = Math.max(1, totalPages);
    const pages: number[] = [];
    const start = Math.max(1, page - 2);
    const end = Math.min(safeTotalPages, page + 2);
    for (let i = start; i <= end; i += 1) {
        pages.push(i);
    }

    return (
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-ui-line bg-ui-subtle/60 px-4 py-3 text-[13px] text-ui-sub">
            <div className="flex flex-wrap items-center gap-3">
                <span>
                    共 <span className="tnum font-medium text-ui-text">{total}</span> {unit} · 第{' '}
                    <span className="tnum font-medium text-ui-text">{page}</span> /{' '}
                    <span className="tnum">{safeTotalPages}</span> 页
                </span>
                {perPageOptions && perPageOptions.length > 0 && onPerPageChange ? (
                    <label className="inline-flex items-center gap-1.5 text-[12.5px] text-ui-sub">
                        <span>每页</span>
                        <select
                            aria-label="每页显示条数"
                            value={perPage}
                            onChange={(event) => onPerPageChange(Number(event.target.value))}
                            className="tnum h-8 rounded-lg border border-ui-line bg-white px-2 text-[12.5px] text-ui-text transition-colors hover:border-ui-line-strong focus:border-accent focus:outline-none"
                        >
                            {perPageOptions.map((option) => (
                                <option key={option} value={option}>
                                    {option} {unit}
                                </option>
                            ))}
                        </select>
                    </label>
                ) : null}
            </div>
            <nav aria-label="分页" className="flex items-center gap-1">
                <button
                    type="button"
                    disabled={page <= 1}
                    onClick={() => onPageChange(page - 1)}
                    className="inline-flex h-8 items-center gap-1 rounded-lg border border-ui-line bg-white px-2.5 text-[13px] text-ui-text transition-colors hover:bg-ui-bg disabled:pointer-events-none disabled:opacity-45"
                >
                    <ChevronLeft className="h-3.5 w-3.5" />
                    上一页
                </button>
                {pages.map((p) => (
                    <button
                        key={p}
                        type="button"
                        aria-current={p === page ? 'page' : undefined}
                        onClick={() => onPageChange(p)}
                        className={cn(
                            'tnum grid h-8 min-w-8 place-items-center rounded-lg px-2 text-[13px] transition-colors',
                            p === page
                                ? 'bg-accent font-medium text-white shadow-xs'
                                : 'text-ui-text hover:bg-[#E7EFED]',
                        )}
                    >
                        {p}
                    </button>
                ))}
                <button
                    type="button"
                    disabled={page >= safeTotalPages}
                    onClick={() => onPageChange(page + 1)}
                    className="inline-flex h-8 items-center gap-1 rounded-lg border border-ui-line bg-white px-2.5 text-[13px] text-ui-text transition-colors hover:bg-ui-bg disabled:pointer-events-none disabled:opacity-45"
                >
                    下一页
                    <ChevronRight className="h-3.5 w-3.5" />
                </button>
            </nav>
        </div>
    );
}
