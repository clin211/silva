import { Check, ChevronDown } from 'lucide-react';
import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/cn';

export interface SelectOption {
    value: string;
    label: string;
    disabled?: boolean;
}

export interface SelectProps {
    id?: string;
    value: string;
    onChange: (value: string) => void;
    options: readonly SelectOption[];
    placeholder?: string;
    ariaLabel?: string;
    disabled?: boolean;
    className?: string;
    menuClassName?: string;
}

interface MenuPosition {
    top: number;
    left: number;
    minWidth: number;
    openUpward: boolean;
}

export function Select({
    id,
    value,
    onChange,
    options,
    placeholder = '请选择',
    ariaLabel,
    disabled = false,
    className,
    menuClassName,
}: SelectProps) {
    const generatedId = useId();
    const triggerId = id ?? generatedId;
    const menuId = `${triggerId}-menu`;

    const triggerRef = useRef<HTMLButtonElement>(null);
    const menuRef = useRef<HTMLDivElement>(null);
    const optionRefs = useRef<(HTMLButtonElement | null)[]>([]);

    const [open, setOpen] = useState(false);
    const [position, setPosition] = useState<MenuPosition | null>(null);
    const [highlightedIndex, setHighlightedIndex] = useState<number>(-1);

    const selectedIndex = options.findIndex((opt) => opt.value === value);
    const selectedOption = selectedIndex >= 0 ? options[selectedIndex] : undefined;
    const hasActiveFilter = value !== '';

    const computePosition = useCallback((): MenuPosition | null => {
        const el = triggerRef.current;
        if (!el) return null;
        const rect = el.getBoundingClientRect();
        const estimatedHeight = Math.min(options.length * 36 + 16, 272);
        const spaceBelow = window.innerHeight - rect.bottom;
        const spaceAbove = rect.top;
        const openUpward = spaceBelow < estimatedHeight && spaceAbove > spaceBelow;

        const minWidth = Math.max(rect.width, 148);
        const maxLeft = Math.max(8, window.innerWidth - minWidth - 8);
        const left = Math.min(Math.max(8, rect.left), maxLeft);
        const top = openUpward ? rect.top - 6 : rect.bottom + 6;

        return { top, left, minWidth, openUpward };
    }, [options.length]);

    const openMenu = useCallback(
        (initialIndex?: number) => {
            if (disabled) return;
            const nextPos = computePosition();
            setPosition(nextPos);
            const fallbackIndex = selectedIndex >= 0 ? selectedIndex : 0;
            setHighlightedIndex(initialIndex ?? fallbackIndex);
            setOpen(true);
        },
        [computePosition, disabled, selectedIndex],
    );

    const closeMenu = useCallback((restoreFocus = false) => {
        setOpen(false);
        if (restoreFocus) {
            triggerRef.current?.focus();
        }
    }, []);

    const selectAt = useCallback(
        (index: number) => {
            const option = options[index];
            if (!option || option.disabled) return;
            onChange(option.value);
            closeMenu(true);
        },
        [closeMenu, onChange, options],
    );

    // Reposition & close on outside click / Escape / scroll
    useEffect(() => {
        if (!open) return;

        const onPointerDown = (event: MouseEvent) => {
            const target = event.target as Node;
            if (!triggerRef.current?.contains(target) && !menuRef.current?.contains(target)) {
                closeMenu(false);
            }
        };

        const onWindowChange = () => {
            const nextPos = computePosition();
            if (nextPos) setPosition(nextPos);
        };

        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.preventDefault();
                closeMenu(true);
                return;
            }
            if (event.key === 'Tab') {
                closeMenu(false);
                return;
            }
            if (event.key === 'ArrowDown') {
                event.preventDefault();
                setHighlightedIndex((prev) => {
                    for (let step = 1; step <= options.length; step++) {
                        const idx = (prev + step) % options.length;
                        if (!options[idx]?.disabled) return idx;
                    }
                    return prev;
                });
                return;
            }
            if (event.key === 'ArrowUp') {
                event.preventDefault();
                setHighlightedIndex((prev) => {
                    for (let step = 1; step <= options.length; step++) {
                        const idx = (prev - step + options.length) % options.length;
                        if (!options[idx]?.disabled) return idx;
                    }
                    return prev;
                });
                return;
            }
            if (event.key === 'Home') {
                event.preventDefault();
                const first = options.findIndex((opt) => !opt.disabled);
                if (first >= 0) setHighlightedIndex(first);
                return;
            }
            if (event.key === 'End') {
                event.preventDefault();
                for (let i = options.length - 1; i >= 0; i--) {
                    if (!options[i]?.disabled) {
                        setHighlightedIndex(i);
                        break;
                    }
                }
                return;
            }
            if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                if (highlightedIndex >= 0) {
                    selectAt(highlightedIndex);
                }
            }
        };

        document.addEventListener('mousedown', onPointerDown);
        document.addEventListener('keydown', onKeyDown);
        window.addEventListener('resize', onWindowChange);
        window.addEventListener('scroll', onWindowChange, true);

        return () => {
            document.removeEventListener('mousedown', onPointerDown);
            document.removeEventListener('keydown', onKeyDown);
            window.removeEventListener('resize', onWindowChange);
            window.removeEventListener('scroll', onWindowChange, true);
        };
    }, [closeMenu, computePosition, highlightedIndex, open, options, selectAt]);

    // Scroll highlighted option into view inside menu
    useEffect(() => {
        if (!open || highlightedIndex < 0) return;
        optionRefs.current[highlightedIndex]?.scrollIntoView({ block: 'nearest' });
    }, [highlightedIndex, open]);

    return (
        <>
            <button
                ref={triggerRef}
                id={triggerId}
                type="button"
                disabled={disabled}
                aria-label={ariaLabel}
                aria-haspopup="menu"
                aria-expanded={open}
                aria-controls={open ? menuId : undefined}
                onClick={() => (open ? closeMenu(false) : openMenu())}
                onKeyDown={(event) => {
                    if (!open && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
                        event.preventDefault();
                        openMenu();
                    }
                }}
                className={cn(
                    'inline-flex h-9 items-center justify-between gap-2.5 rounded-[9px] border bg-white px-3 text-[13.5px] leading-none transition-[border-color,box-shadow,background-color] duration-150',
                    open
                        ? 'border-accent ring-[3px] ring-accent/15'
                        : hasActiveFilter
                          ? 'border-accent/45 bg-accent-tint/35 text-accent-2 hover:border-accent/70'
                          : 'border-ui-line text-ui-text hover:border-ui-line-strong hover:bg-ui-subtle/50',
                    disabled && 'cursor-not-allowed bg-ui-subtle text-ui-sub opacity-60',
                    className,
                )}
            >
                <span className="truncate font-normal">
                    {selectedOption ? selectedOption.label : placeholder}
                </span>
                <ChevronDown
                    aria-hidden="true"
                    className={cn(
                        'h-3.5 w-3.5 shrink-0 transition-transform duration-150',
                        hasActiveFilter ? 'text-accent' : 'text-ui-sub',
                        open && 'rotate-180 text-accent',
                    )}
                />
            </button>

            {open && position
                ? createPortal(
                      <div
                          ref={menuRef}
                          id={menuId}
                          role="menu"
                          aria-labelledby={triggerId}
                          style={{
                              position: 'fixed',
                              top: position.top,
                              left: position.left,
                              minWidth: position.minWidth,
                              transform: position.openUpward ? 'translateY(-100%)' : undefined,
                          }}
                          className={cn(
                              'animate-fade-in z-[70] max-h-[264px] max-w-[280px] overflow-y-auto rounded-xl border border-ui-line bg-white p-1.5 shadow-[0_14px_34px_-8px_rgba(17,25,23,0.18),0_4px_12px_-4px_rgba(17,25,23,0.06)]',
                              menuClassName,
                          )}
                      >
                          {options.map((option, index) => {
                              const isSelected = option.value === value;
                              const isHighlighted = index === highlightedIndex;
                              return (
                                  <button
                                      key={option.value}
                                      ref={(el) => {
                                          optionRefs.current[index] = el;
                                      }}
                                      type="button"
                                      role="menuitemradio"
                                      aria-checked={isSelected}
                                      disabled={option.disabled}
                                      onMouseEnter={() => setHighlightedIndex(index)}
                                      onClick={() => selectAt(index)}
                                      className={cn(
                                          'flex w-full items-center justify-between gap-3 rounded-lg px-2.5 py-2 text-left text-[13.5px] leading-snug transition-colors',
                                          isSelected
                                              ? 'bg-accent-tint/90 font-medium text-accent-2'
                                              : isHighlighted
                                                ? 'bg-[#F2F6F4] text-ui-text'
                                                : 'text-ui-text hover:bg-[#F2F6F4]',
                                          option.disabled &&
                                              'cursor-not-allowed text-ui-sub opacity-45',
                                      )}
                                  >
                                      <span className="truncate">{option.label}</span>
                                      {isSelected ? (
                                          <Check
                                              aria-hidden="true"
                                              className="h-3.5 w-3.5 shrink-0 text-accent"
                                              strokeWidth={2.5}
                                          />
                                      ) : null}
                                  </button>
                              );
                          })}
                      </div>,
                      document.body,
                  )
                : null}
        </>
    );
}
