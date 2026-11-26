import type { ReactNode } from 'react';
import { cn } from '@/lib/cn';

export type ChipTone = 'ok' | 'warn' | 'bad' | 'info' | 'mute';

const TONE_CLASS: Record<ChipTone, string> = {
    ok: 'chip-ok',
    warn: 'chip-warn',
    bad: 'chip-bad',
    info: 'chip-info',
    mute: 'chip-mute',
};

export function StatusChip({
    tone,
    children,
    className,
}: {
    tone: ChipTone;
    children: ReactNode;
    className?: string;
}) {
    return <span className={cn('chip', TONE_CLASS[tone], className)}>{children}</span>;
}
