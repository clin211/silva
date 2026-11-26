import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/cn';

export type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'danger-outline' | 'ok' | 'ghost';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ButtonVariant;
    size?: ButtonSize;
    children: ReactNode;
}

const VARIANT_CLASSES: Record<ButtonVariant, string> = {
    primary:
        'bg-accent text-white hover:bg-accent-2 border border-accent-2/20 font-medium shadow-[0_1px_2px_rgba(19,122,77,0.28),inset_0_1px_0_rgba(255,255,255,0.18)]',
    secondary:
        'bg-white text-ui-text border border-ui-line hover:bg-ui-subtle hover:border-ui-line-strong font-medium shadow-[0_1px_2px_rgba(16,37,35,0.04)]',
    danger: 'bg-bad text-white hover:bg-[#961d14] border border-transparent font-medium shadow-[0_1px_2px_rgba(180,35,24,0.25)]',
    'danger-outline': 'bg-white text-bad border border-[#EBC5C0] hover:bg-bad-tint font-medium',
    ok: 'bg-ok text-white hover:bg-[#0b6646] border border-transparent font-medium shadow-[0_1px_2px_rgba(14,124,85,0.25)]',
    ghost: 'bg-transparent text-ui-sub hover:bg-[#E7EFED] hover:text-ui-text border border-transparent font-medium',
};

const SIZE_CLASSES: Record<ButtonSize, string> = {
    sm: 'h-8 px-3 text-[13px] rounded-[8px]',
    md: 'h-9 px-3.5 text-[13.5px] rounded-[9px]',
    lg: 'h-10 px-5 text-[14.5px] rounded-[10px]',
};

export function Button({
    variant = 'secondary',
    size = 'md',
    className,
    type = 'button',
    ...props
}: ButtonProps) {
    return (
        <button
            type={type}
            className={cn(
                'inline-flex items-center justify-center gap-1.5 whitespace-nowrap transition-[background-color,border-color,color,box-shadow,transform] duration-150 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-45',
                VARIANT_CLASSES[variant],
                SIZE_CLASSES[size],
                className,
            )}
            {...props}
        />
    );
}
