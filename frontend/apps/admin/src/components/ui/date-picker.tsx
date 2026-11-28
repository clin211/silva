import {
    ArrowRight,
    Calendar,
    ChevronLeft,
    ChevronRight,
    ChevronsLeft,
    ChevronsRight,
    X,
} from 'lucide-react';
import {
    useCallback,
    useEffect,
    useId,
    useMemo,
    useRef,
    useState,
    type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/cn';

// ---------------------------------------------------------------------------
// Timezone-safe YYYY-MM-DD calendar utilities
// ---------------------------------------------------------------------------

interface DateParts {
    year: number;
    month: number; // 1..12
    day: number; // 1..31
}

interface YearMonth {
    year: number;
    month: number; // 1..12
}

interface CalendarCell {
    iso: string;
    day: number;
    inCurrentMonth: boolean;
}

const WEEKDAY_LABELS = ['一', '二', '三', '四', '五', '六', '日'] as const;
const MONTH_LABELS = [
    '1月',
    '2月',
    '3月',
    '4月',
    '5月',
    '6月',
    '7月',
    '8月',
    '9月',
    '10月',
    '11月',
    '12月',
] as const;

function pad2(n: number): string {
    return String(n).padStart(2, '0');
}

export function formatIsoDate(parts: DateParts): string {
    return `${String(parts.year).padStart(4, '0')}-${pad2(parts.month)}-${pad2(parts.day)}`;
}

export function parseIsoDate(value: string | null | undefined): DateParts | null {
    if (!value) return null;
    const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value.trim());
    if (!match) return null;
    const year = Number(match[1]);
    const month = Number(match[2]);
    const day = Number(match[3]);
    if (month < 1 || month > 12 || day < 1) return null;
    const maxDay = getDaysInMonth(year, month);
    if (day > maxDay) return null;
    return { year, month, day };
}

function getDaysInMonth(year: number, month: number): number {
    return new Date(year, month, 0).getDate();
}

export function getTodayIso(): string {
    const now = new Date();
    return formatIsoDate({
        year: now.getFullYear(),
        month: now.getMonth() + 1,
        day: now.getDate(),
    });
}

function addDaysIso(iso: string, delta: number): string {
    const parsed = parseIsoDate(iso);
    if (!parsed) return iso;
    const next = new Date(parsed.year, parsed.month - 1, parsed.day + delta);
    return formatIsoDate({
        year: next.getFullYear(),
        month: next.getMonth() + 1,
        day: next.getDate(),
    });
}

function addMonths(ym: YearMonth, delta: number): YearMonth {
    const total = ym.year * 12 + (ym.month - 1) + delta;
    const year = Math.floor(total / 12);
    const month = (((total % 12) + 12) % 12) + 1;
    return { year, month };
}

function diffDaysInclusive(from: string, to: string): number | null {
    const a = parseIsoDate(from);
    const b = parseIsoDate(to);
    if (!a || !b) return null;
    const utcA = Date.UTC(a.year, a.month - 1, a.day);
    const utcB = Date.UTC(b.year, b.month - 1, b.day);
    return Math.round((utcB - utcA) / 86_400_000) + 1;
}

function isDateDisabled(iso: string, min?: string, max?: string): boolean {
    if (min && iso < min) return true;
    if (max && iso > max) return true;
    return false;
}

/**
 * 构建 6 行 × 7 列（周一至周日）的固定高度月历网格，避免翻月时弹窗跳动。
 */
function buildMonthGrid(year: number, month: number): CalendarCell[] {
    const firstWeekday = (new Date(year, month - 1, 1).getDay() + 6) % 7; // Mon=0..Sun=6
    const daysInMonth = getDaysInMonth(year, month);
    const prev = addMonths({ year, month }, -1);
    const daysInPrevMonth = getDaysInMonth(prev.year, prev.month);
    const next = addMonths({ year, month }, 1);

    const cells: CalendarCell[] = [];

    for (let i = firstWeekday - 1; i >= 0; i--) {
        const day = daysInPrevMonth - i;
        cells.push({
            iso: formatIsoDate({ year: prev.year, month: prev.month, day }),
            day,
            inCurrentMonth: false,
        });
    }

    for (let day = 1; day <= daysInMonth; day++) {
        cells.push({
            iso: formatIsoDate({ year, month, day }),
            day,
            inCurrentMonth: true,
        });
    }

    let nextDay = 1;
    while (cells.length < 42) {
        cells.push({
            iso: formatIsoDate({ year: next.year, month: next.month, day: nextDay }),
            day: nextDay,
            inCurrentMonth: false,
        });
        nextDay += 1;
    }

    return cells;
}

// ---------------------------------------------------------------------------
// Shared popover positioning hook
// ---------------------------------------------------------------------------

interface PopoverPosition {
    top: number;
    left: number;
    openUpward: boolean;
}

function computePopoverPosition(
    triggerEl: HTMLElement | null,
    estimatedWidth: number,
    estimatedHeight: number,
): PopoverPosition | null {
    if (!triggerEl) return null;
    const rect = triggerEl.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom;
    const spaceAbove = rect.top;
    const openUpward = spaceBelow < estimatedHeight && spaceAbove > spaceBelow;

    const clampedWidth = Math.min(estimatedWidth, window.innerWidth - 16);
    const maxLeft = Math.max(8, window.innerWidth - clampedWidth - 8);
    const left = Math.min(Math.max(8, rect.left), maxLeft);
    const top = openUpward ? rect.top - 6 : rect.bottom + 6;

    return { top, left, openUpward };
}

function useMediaMinWidth(minWidth: number): boolean {
    const [matches, setMatches] = useState(() =>
        typeof window !== 'undefined' ? window.innerWidth >= minWidth : true,
    );

    useEffect(() => {
        const onResize = () => setMatches(window.innerWidth >= minWidth);
        window.addEventListener('resize', onResize);
        return () => window.removeEventListener('resize', onResize);
    }, [minWidth]);

    return matches;
}

// ---------------------------------------------------------------------------
// DatePicker (Single Date)
// ---------------------------------------------------------------------------

type CalendarView = 'days' | 'months' | 'years';

export interface DatePickerProps {
    id?: string;
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    ariaLabel?: string;
    disabled?: boolean;
    clearable?: boolean;
    min?: string;
    max?: string;
    variant?: 'field' | 'filter';
    className?: string;
    panelClassName?: string;
}

export function DatePicker({
    id,
    value,
    onChange,
    placeholder = '选择日期',
    ariaLabel,
    disabled = false,
    clearable = true,
    min,
    max,
    variant = 'field',
    className,
    panelClassName,
}: DatePickerProps) {
    const generatedId = useId();
    const triggerId = id ?? generatedId;
    const panelId = `${triggerId}-calendar`;

    const triggerRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDialogElement>(null);

    const todayIso = getTodayIso();
    const parsedValue = parseIsoDate(value);

    const [open, setOpen] = useState(false);
    const [position, setPosition] = useState<PopoverPosition | null>(null);
    const [view, setView] = useState<CalendarView>('days');
    const [viewMonth, setViewMonth] = useState<YearMonth>(() => {
        const initial = parsedValue ?? parseIsoDate(todayIso)!;
        return { year: initial.year, month: initial.month };
    });

    const hasValue = Boolean(parsedValue);
    const showActiveFilter = variant === 'filter' && hasValue;

    const updatePosition = useCallback(() => {
        return computePopoverPosition(triggerRef.current, 296, 368);
    }, []);

    const openCalendar = useCallback(() => {
        if (disabled) return;
        const anchor = parseIsoDate(value) ?? parseIsoDate(getTodayIso())!;
        setViewMonth({ year: anchor.year, month: anchor.month });
        setView('days');
        setPosition(updatePosition());
        setOpen(true);
    }, [disabled, updatePosition, value]);

    const closeCalendar = useCallback((restoreFocus = false) => {
        setOpen(false);
        if (restoreFocus) {
            triggerRef.current?.focus();
        }
    }, []);

    const handleSelectIso = useCallback(
        (iso: string) => {
            if (isDateDisabled(iso, min, max)) return;
            onChange(iso);
            closeCalendar(true);
        },
        [closeCalendar, max, min, onChange],
    );

    useEffect(() => {
        if (!open) return;

        const onPointerDown = (event: MouseEvent) => {
            const target = event.target as Node;
            if (!triggerRef.current?.contains(target) && !panelRef.current?.contains(target)) {
                closeCalendar(false);
            }
        };

        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.preventDefault();
                closeCalendar(true);
            }
        };

        const onWindowChange = () => {
            const nextPos = updatePosition();
            if (nextPos) setPosition(nextPos);
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
    }, [closeCalendar, open, updatePosition]);

    const cells = useMemo(
        () => buildMonthGrid(viewMonth.year, viewMonth.month),
        [viewMonth.year, viewMonth.month],
    );

    const decadeStart = Math.floor(viewMonth.year / 10) * 10;
    const yearsGrid = useMemo(
        () => Array.from({ length: 12 }, (_, idx) => decadeStart - 1 + idx),
        [decadeStart],
    );

    const canSelectToday = !isDateDisabled(todayIso, min, max);

    return (
        <div className={cn('relative inline-flex w-full items-center', className)}>
            <button
                ref={triggerRef}
                id={triggerId}
                type="button"
                disabled={disabled}
                aria-label={ariaLabel}
                aria-haspopup="dialog"
                aria-expanded={open}
                aria-controls={open ? panelId : undefined}
                onClick={() => (open ? closeCalendar(false) : openCalendar())}
                onKeyDown={(event) => {
                    if (!open && event.key === 'ArrowDown') {
                        event.preventDefault();
                        openCalendar();
                    }
                }}
                className={cn(
                    'inline-flex h-9 w-full items-center gap-2 rounded-[9px] border bg-white px-3 text-left text-[13.5px] leading-none transition-[border-color,box-shadow,background-color] duration-150',
                    clearable && hasValue && !disabled ? 'pr-8' : 'pr-3',
                    open
                        ? 'border-accent ring-[3px] ring-accent/15'
                        : showActiveFilter
                          ? 'border-accent/45 bg-accent-tint/35 text-accent-2 hover:border-accent/70'
                          : 'border-ui-line text-ui-text hover:border-ui-line-strong hover:bg-ui-subtle/50',
                    disabled && 'cursor-not-allowed bg-ui-subtle text-ui-sub opacity-60',
                )}
            >
                <Calendar
                    aria-hidden="true"
                    className={cn(
                        'h-3.5 w-3.5 shrink-0 transition-colors',
                        open || showActiveFilter ? 'text-accent' : 'text-ui-sub',
                    )}
                />
                <span
                    className={cn(
                        'tnum flex-1 truncate',
                        hasValue
                            ? showActiveFilter
                                ? 'font-medium text-accent-2'
                                : 'text-ui-text'
                            : 'text-[#889b98]',
                    )}
                >
                    {hasValue ? value : placeholder}
                </span>
            </button>

            {clearable && hasValue && !disabled ? (
                <button
                    type="button"
                    aria-label="清除日期"
                    onClick={() => {
                        onChange('');
                        closeCalendar(false);
                    }}
                    className="absolute top-1/2 right-2 inline-flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded-full text-ui-sub transition-colors hover:bg-ui-line/80 hover:text-ui-text"
                >
                    <X className="h-3 w-3" />
                </button>
            ) : null}

            {open && position
                ? createPortal(
                      <dialog
                          ref={panelRef}
                          id={panelId}
                          open
                          aria-label={ariaLabel ?? '日期选择器'}
                          style={{
                              position: 'fixed',
                              top: position.top,
                              left: position.left,
                              right: 'auto',
                              transform: position.openUpward ? 'translateY(-100%)' : undefined,
                          }}
                          className={cn(
                              'animate-fade-in z-[70] m-0 w-[296px] overflow-hidden rounded-2xl border border-ui-line bg-white p-0 text-ui-text shadow-[0_18px_44px_-10px_rgba(17,25,23,0.18),0_4px_14px_-4px_rgba(17,25,23,0.06)]',
                              panelClassName,
                          )}
                      >
                          {/* Header */}
                          <div className="flex items-center justify-between gap-1 border-b border-ui-line/70 px-3 py-2.5">
                              <button
                                  type="button"
                                  aria-label={
                                      view === 'years'
                                          ? '上一个十年'
                                          : view === 'months'
                                            ? '上一年'
                                            : '上个月'
                                  }
                                  onClick={() => {
                                      if (view === 'years') {
                                          setViewMonth((prev) => ({
                                              ...prev,
                                              year: prev.year - 10,
                                          }));
                                      } else if (view === 'months') {
                                          setViewMonth((prev) => ({
                                              ...prev,
                                              year: prev.year - 1,
                                          }));
                                      } else {
                                          setViewMonth((prev) => addMonths(prev, -1));
                                      }
                                  }}
                                  className="inline-flex h-7 w-7 items-center justify-center rounded-lg text-ui-sub transition-colors hover:bg-[#F2F6F4] hover:text-ui-text"
                              >
                                  <ChevronLeft className="h-4 w-4" />
                              </button>

                              <div className="flex items-center gap-0.5 text-[13.5px] font-semibold text-ui-text">
                                  {view === 'years' ? (
                                      <span className="tnum px-2 py-1 text-ui-text">
                                          {decadeStart} – {decadeStart + 9}
                                      </span>
                                  ) : (
                                      <>
                                          <button
                                              type="button"
                                              onClick={() => setView('years')}
                                              className="tnum rounded-md px-1.5 py-1 transition-colors hover:bg-[#F2F6F4] hover:text-accent"
                                          >
                                              {viewMonth.year}年
                                          </button>
                                          {view === 'days' ? (
                                              <button
                                                  type="button"
                                                  onClick={() => setView('months')}
                                                  className="tnum rounded-md px-1.5 py-1 transition-colors hover:bg-[#F2F6F4] hover:text-accent"
                                              >
                                                  {viewMonth.month}月
                                              </button>
                                          ) : null}
                                      </>
                                  )}
                              </div>

                              <button
                                  type="button"
                                  aria-label={
                                      view === 'years'
                                          ? '下一个十年'
                                          : view === 'months'
                                            ? '下一年'
                                            : '下个月'
                                  }
                                  onClick={() => {
                                      if (view === 'years') {
                                          setViewMonth((prev) => ({
                                              ...prev,
                                              year: prev.year + 10,
                                          }));
                                      } else if (view === 'months') {
                                          setViewMonth((prev) => ({
                                              ...prev,
                                              year: prev.year + 1,
                                          }));
                                      } else {
                                          setViewMonth((prev) => addMonths(prev, 1));
                                      }
                                  }}
                                  className="inline-flex h-7 w-7 items-center justify-center rounded-lg text-ui-sub transition-colors hover:bg-[#F2F6F4] hover:text-ui-text"
                              >
                                  <ChevronRight className="h-4 w-4" />
                              </button>
                          </div>

                          {/* Body */}
                          <div className="p-3">
                              {view === 'days' ? (
                                  <>
                                      <div className="mb-1.5 grid grid-cols-7 text-center">
                                          {WEEKDAY_LABELS.map((label) => (
                                              <span
                                                  key={label}
                                                  className="py-1 text-[11.5px] font-medium text-ui-sub/80"
                                              >
                                                  {label}
                                              </span>
                                          ))}
                                      </div>

                                      <div className="grid grid-cols-7 gap-y-0.5">
                                          {cells.map((cell) => {
                                              const isSelected = cell.iso === value;
                                              const isToday = cell.iso === todayIso;
                                              const cellDisabled = isDateDisabled(
                                                  cell.iso,
                                                  min,
                                                  max,
                                              );

                                              return (
                                                  <div
                                                      key={cell.iso}
                                                      className="flex items-center justify-center"
                                                  >
                                                      <button
                                                          type="button"
                                                          disabled={cellDisabled}
                                                          aria-label={cell.iso}
                                                          aria-pressed={isSelected}
                                                          onClick={() => handleSelectIso(cell.iso)}
                                                          className={cn(
                                                              'tnum relative inline-flex h-8 w-8 items-center justify-center rounded-lg text-[13px] transition-colors',
                                                              isSelected
                                                                  ? 'bg-accent font-semibold text-white shadow-[0_2px_6px_rgba(19,122,77,0.3)]'
                                                                  : cellDisabled
                                                                    ? 'cursor-not-allowed text-ui-sub/30'
                                                                    : cell.inCurrentMonth
                                                                      ? isToday
                                                                          ? 'font-semibold text-accent ring-1 ring-inset ring-accent/40 hover:bg-accent-tint/70'
                                                                          : 'text-ui-text hover:bg-[#F0F5F2]'
                                                                      : 'text-ui-sub/40 hover:bg-[#F4F7F5] hover:text-ui-sub/75',
                                                          )}
                                                      >
                                                          {cell.day}
                                                          {isToday && !isSelected ? (
                                                              <span
                                                                  aria-hidden="true"
                                                                  className="absolute bottom-1 h-1 w-1 rounded-full bg-accent"
                                                              />
                                                          ) : null}
                                                      </button>
                                                  </div>
                                              );
                                          })}
                                      </div>
                                  </>
                              ) : view === 'months' ? (
                                  <div className="grid grid-cols-3 gap-2 py-2">
                                      {MONTH_LABELS.map((label, idx) => {
                                          const monthNum = idx + 1;
                                          const isCurrent =
                                              parsedValue?.year === viewMonth.year &&
                                              parsedValue?.month === monthNum;
                                          return (
                                              <button
                                                  key={label}
                                                  type="button"
                                                  onClick={() => {
                                                      setViewMonth((prev) => ({
                                                          ...prev,
                                                          month: monthNum,
                                                      }));
                                                      setView('days');
                                                  }}
                                                  className={cn(
                                                      'tnum h-10 rounded-xl text-[13px] font-medium transition-colors',
                                                      isCurrent
                                                          ? 'bg-accent text-white shadow-[0_2px_6px_rgba(19,122,77,0.28)]'
                                                          : 'text-ui-text hover:bg-[#F0F5F2]',
                                                  )}
                                              >
                                                  {label}
                                              </button>
                                          );
                                      })}
                                  </div>
                              ) : (
                                  <div className="grid grid-cols-3 gap-2 py-2">
                                      {yearsGrid.map((year) => {
                                          const isSelectedYear = parsedValue?.year === year;
                                          const inDecade =
                                              year >= decadeStart && year <= decadeStart + 9;
                                          return (
                                              <button
                                                  key={year}
                                                  type="button"
                                                  onClick={() => {
                                                      setViewMonth((prev) => ({ ...prev, year }));
                                                      setView('months');
                                                  }}
                                                  className={cn(
                                                      'tnum h-10 rounded-xl text-[13px] font-medium transition-colors',
                                                      isSelectedYear
                                                          ? 'bg-accent text-white shadow-[0_2px_6px_rgba(19,122,77,0.28)]'
                                                          : inDecade
                                                            ? 'text-ui-text hover:bg-[#F0F5F2]'
                                                            : 'text-ui-sub/45 hover:bg-[#F4F7F5]',
                                                  )}
                                              >
                                                  {year}
                                              </button>
                                          );
                                      })}
                                  </div>
                              )}
                          </div>

                          {/* Footer */}
                          <div className="flex items-center justify-between border-t border-ui-line/80 bg-ui-subtle/60 px-3.5 py-2">
                              <button
                                  type="button"
                                  disabled={!canSelectToday}
                                  onClick={() => handleSelectIso(todayIso)}
                                  className="rounded-md px-2 py-1 text-[12.5px] font-medium text-accent transition-colors hover:bg-accent-tint/80 disabled:pointer-events-none disabled:opacity-40"
                              >
                                  今天
                              </button>
                              {clearable && hasValue ? (
                                  <button
                                      type="button"
                                      onClick={() => {
                                          onChange('');
                                          closeCalendar(true);
                                      }}
                                      className="rounded-md px-2 py-1 text-[12.5px] font-medium text-ui-sub transition-colors hover:bg-ui-line/60 hover:text-ui-text"
                                  >
                                      清除
                                  </button>
                              ) : (
                                  <span className="tnum text-[11.5px] text-ui-sub">
                                      {hasValue ? value : 'YYYY-MM-DD'}
                                  </span>
                              )}
                          </div>
                      </dialog>,
                      document.body,
                  )
                : null}
        </div>
    );
}

// ---------------------------------------------------------------------------
// DateRangePicker (Dual-month + Quick Presets for Filter Toolbars)
// ---------------------------------------------------------------------------

export interface DateRangeValue {
    from: string;
    to: string;
}

export interface DateRangePickerProps {
    id?: string;
    from: string;
    to: string;
    onChange: (range: DateRangeValue) => void;
    placeholder?: string;
    ariaLabel?: string;
    disabled?: boolean;
    min?: string;
    max?: string;
    className?: string;
    panelClassName?: string;
}

interface RangePreset {
    id: string;
    label: string;
    getRange: (todayIso: string) => DateRangeValue;
}

const RANGE_PRESETS: readonly RangePreset[] = [
    {
        id: 'today',
        label: '今天',
        getRange: (today) => ({ from: today, to: today }),
    },
    {
        id: 'yesterday',
        label: '昨天',
        getRange: (today) => {
            const y = addDaysIso(today, -1);
            return { from: y, to: y };
        },
    },
    {
        id: 'last-7',
        label: '近 7 天',
        getRange: (today) => ({ from: addDaysIso(today, -6), to: today }),
    },
    {
        id: 'last-30',
        label: '近 30 天',
        getRange: (today) => ({ from: addDaysIso(today, -29), to: today }),
    },
    {
        id: 'this-month',
        label: '本月',
        getRange: (today) => {
            const parsed = parseIsoDate(today)!;
            return {
                from: formatIsoDate({ year: parsed.year, month: parsed.month, day: 1 }),
                to: today,
            };
        },
    },
    {
        id: 'last-month',
        label: '上月',
        getRange: (today) => {
            const parsed = parseIsoDate(today)!;
            const prev = addMonths({ year: parsed.year, month: parsed.month }, -1);
            return {
                from: formatIsoDate({ year: prev.year, month: prev.month, day: 1 }),
                to: formatIsoDate({
                    year: prev.year,
                    month: prev.month,
                    day: getDaysInMonth(prev.year, prev.month),
                }),
            };
        },
    },
];

function computeInitialLeftMonth(from: string, to: string, dualMonth: boolean): YearMonth {
    const parsedFrom = parseIsoDate(from);
    if (parsedFrom) {
        return { year: parsedFrom.year, month: parsedFrom.month };
    }
    const parsedTo = parseIsoDate(to);
    if (parsedTo) {
        return dualMonth
            ? addMonths({ year: parsedTo.year, month: parsedTo.month }, -1)
            : { year: parsedTo.year, month: parsedTo.month };
    }
    const today = parseIsoDate(getTodayIso())!;
    return dualMonth
        ? addMonths({ year: today.year, month: today.month }, -1)
        : { year: today.year, month: today.month };
}

export function DateRangePicker({
    id,
    from,
    to,
    onChange,
    placeholder = '日期范围',
    ariaLabel = '选择日期范围',
    disabled = false,
    min,
    max,
    className,
    panelClassName,
}: DateRangePickerProps) {
    const generatedId = useId();
    const triggerId = id ?? generatedId;
    const panelId = `${triggerId}-range-calendar`;

    const triggerRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDialogElement>(null);

    const isDesktop = useMediaMinWidth(680);
    const todayIso = getTodayIso();

    const [open, setOpen] = useState(false);
    const [position, setPosition] = useState<PopoverPosition | null>(null);
    const [leftMonth, setLeftMonth] = useState<YearMonth>(() =>
        computeInitialLeftMonth(from, to, isDesktop),
    );
    // When picking a new range: first click sets draftStart, second click commits range
    const [draftStart, setDraftStart] = useState<string | null>(null);
    const [hoveredIso, setHoveredIso] = useState<string | null>(null);

    const hasValue = Boolean(from || to);

    const updatePosition = useCallback(() => {
        const estimatedWidth = isDesktop ? 672 : 304;
        const estimatedHeight = isDesktop ? 364 : 416;
        return computePopoverPosition(triggerRef.current, estimatedWidth, estimatedHeight);
    }, [isDesktop]);

    const openCalendar = useCallback(() => {
        if (disabled) return;
        setLeftMonth(computeInitialLeftMonth(from, to, isDesktop));
        setDraftStart(null);
        setHoveredIso(null);
        setPosition(updatePosition());
        setOpen(true);
    }, [disabled, from, isDesktop, to, updatePosition]);

    const closeCalendar = useCallback((restoreFocus = false) => {
        setOpen(false);
        setDraftStart(null);
        setHoveredIso(null);
        if (restoreFocus) {
            triggerRef.current?.focus();
        }
    }, []);

    useEffect(() => {
        if (!open) return;

        const onPointerDown = (event: MouseEvent) => {
            const target = event.target as Node;
            if (!triggerRef.current?.contains(target) && !panelRef.current?.contains(target)) {
                closeCalendar(false);
            }
        };

        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.preventDefault();
                closeCalendar(true);
            }
        };

        const onWindowChange = () => {
            const nextPos = updatePosition();
            if (nextPos) setPosition(nextPos);
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
    }, [closeCalendar, open, updatePosition]);

    const handleDayClick = useCallback(
        (iso: string) => {
            if (isDateDisabled(iso, min, max)) return;
            if (draftStart === null) {
                setDraftStart(iso);
                return;
            }
            const [nextFrom, nextTo] = iso < draftStart ? [iso, draftStart] : [draftStart, iso];
            onChange({ from: nextFrom, to: nextTo });
            closeCalendar(true);
        },
        [closeCalendar, draftStart, max, min, onChange],
    );

    const handlePresetClick = useCallback(
        (preset: RangePreset) => {
            const range = preset.getRange(todayIso);
            onChange(range);
            closeCalendar(true);
        },
        [closeCalendar, onChange, todayIso],
    );

    const handleClear = useCallback(() => {
        onChange({ from: '', to: '' });
        closeCalendar(false);
    }, [closeCalendar, onChange]);

    // Compute active visual range (either previewing draft or displaying committed props)
    const visualRange = useMemo(() => {
        if (draftStart !== null) {
            if (hoveredIso && !isDateDisabled(hoveredIso, min, max)) {
                return hoveredIso < draftStart
                    ? { start: hoveredIso, end: draftStart, isPreview: true }
                    : { start: draftStart, end: hoveredIso, isPreview: true };
            }
            return { start: draftStart, end: draftStart, isPreview: true };
        }
        return {
            start: from || null,
            end: to || null,
            isPreview: false,
        };
    }, [draftStart, from, hoveredIso, max, min, to]);

    const rightMonth = useMemo(() => addMonths(leftMonth, 1), [leftMonth]);
    const monthsToRender = useMemo(
        () => (isDesktop ? [leftMonth, rightMonth] : [leftMonth]),
        [isDesktop, leftMonth, rightMonth],
    );

    const committedDays = from && to ? diffDaysInclusive(from, to) : null;

    const handleTriggerKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
        if (!open && (event.key === 'ArrowDown' || event.key === 'Enter')) {
            event.preventDefault();
            openCalendar();
        }
    };

    const renderMonthPanel = (ym: YearMonth, index: number, total: number) => {
        const cells = buildMonthGrid(ym.year, ym.month);
        const showPrev = index === 0;
        const showNext = index === total - 1;

        return (
            <div key={`${ym.year}-${ym.month}`} className="w-[260px]">
                <div className="mb-2 flex items-center justify-between">
                    {showPrev ? (
                        <div className="flex items-center gap-0.5">
                            <button
                                type="button"
                                aria-label="上一年"
                                onClick={() => setLeftMonth((prev) => addMonths(prev, -12))}
                                className="inline-flex h-7 w-7 items-center justify-center rounded-lg text-ui-sub transition-colors hover:bg-[#F2F6F4] hover:text-ui-text"
                            >
                                <ChevronsLeft className="h-3.5 w-3.5" />
                            </button>
                            <button
                                type="button"
                                aria-label="上个月"
                                onClick={() => setLeftMonth((prev) => addMonths(prev, -1))}
                                className="inline-flex h-7 w-7 items-center justify-center rounded-lg text-ui-sub transition-colors hover:bg-[#F2F6F4] hover:text-ui-text"
                            >
                                <ChevronLeft className="h-4 w-4" />
                            </button>
                        </div>
                    ) : (
                        <span className="h-7 w-14" aria-hidden="true" />
                    )}

                    <span className="tnum text-[13.5px] font-semibold text-ui-text">
                        {ym.year}年 {ym.month}月
                    </span>

                    {showNext ? (
                        <div className="flex items-center gap-0.5">
                            <button
                                type="button"
                                aria-label="下个月"
                                onClick={() => setLeftMonth((prev) => addMonths(prev, 1))}
                                className="inline-flex h-7 w-7 items-center justify-center rounded-lg text-ui-sub transition-colors hover:bg-[#F2F6F4] hover:text-ui-text"
                            >
                                <ChevronRight className="h-4 w-4" />
                            </button>
                            <button
                                type="button"
                                aria-label="下一年"
                                onClick={() => setLeftMonth((prev) => addMonths(prev, 12))}
                                className="inline-flex h-7 w-7 items-center justify-center rounded-lg text-ui-sub transition-colors hover:bg-[#F2F6F4] hover:text-ui-text"
                            >
                                <ChevronsRight className="h-3.5 w-3.5" />
                            </button>
                        </div>
                    ) : (
                        <span className="h-7 w-14" aria-hidden="true" />
                    )}
                </div>

                <div className="mb-1 grid grid-cols-7 text-center">
                    {WEEKDAY_LABELS.map((label) => (
                        <span key={label} className="py-1 text-[11.5px] font-medium text-ui-sub/80">
                            {label}
                        </span>
                    ))}
                </div>

                <div
                    className="grid grid-cols-7 gap-y-0.5"
                    onMouseLeave={() => setHoveredIso(null)}
                >
                    {cells.map((cell, cellIdx) => {
                        if (!cell.inCurrentMonth) {
                            return <div key={cell.iso} className="h-8" aria-hidden="true" />;
                        }

                        const cellDisabled = isDateDisabled(cell.iso, min, max);
                        const isStart = visualRange.start === cell.iso;
                        const isEnd = visualRange.end === cell.iso;
                        const isEndpoint = isStart || isEnd;
                        const hasSpan = Boolean(
                            visualRange.start &&
                            visualRange.end &&
                            visualRange.start !== visualRange.end,
                        );
                        const isBetween =
                            hasSpan &&
                            visualRange.start !== null &&
                            visualRange.end !== null &&
                            cell.iso > visualRange.start &&
                            cell.iso < visualRange.end;
                        const isToday = cell.iso === todayIso;
                        const colIndex = cellIdx % 7;

                        return (
                            <div
                                key={cell.iso}
                                className={cn(
                                    'relative flex h-8 items-center justify-center',
                                    hasSpan &&
                                        (isBetween || isStart || isEnd) &&
                                        (visualRange.isPreview
                                            ? 'bg-accent-tint/65'
                                            : 'bg-accent-tint/90'),
                                    hasSpan && (isStart || colIndex === 0) && 'rounded-l-lg',
                                    hasSpan && (isEnd || colIndex === 6) && 'rounded-r-lg',
                                )}
                            >
                                <button
                                    type="button"
                                    disabled={cellDisabled}
                                    aria-label={cell.iso}
                                    aria-pressed={isEndpoint}
                                    onMouseEnter={() => {
                                        if (draftStart !== null) {
                                            setHoveredIso(cell.iso);
                                        }
                                    }}
                                    onClick={() => handleDayClick(cell.iso)}
                                    className={cn(
                                        'tnum relative z-10 inline-flex h-8 w-8 items-center justify-center rounded-lg text-[13px] transition-colors',
                                        isEndpoint
                                            ? 'bg-accent font-semibold text-white shadow-[0_2px_6px_rgba(19,122,77,0.3)]'
                                            : isBetween
                                              ? 'font-medium text-accent-2 hover:bg-accent/15'
                                              : cellDisabled
                                                ? 'cursor-not-allowed text-ui-sub/30'
                                                : isToday
                                                  ? 'font-semibold text-accent ring-1 ring-inset ring-accent/40 hover:bg-accent-tint/70'
                                                  : 'text-ui-text hover:bg-[#F0F5F2]',
                                    )}
                                >
                                    {cell.day}
                                    {isToday && !isEndpoint ? (
                                        <span
                                            aria-hidden="true"
                                            className="absolute bottom-1 h-1 w-1 rounded-full bg-accent"
                                        />
                                    ) : null}
                                </button>
                            </div>
                        );
                    })}
                </div>
            </div>
        );
    };

    return (
        <div className={cn('relative inline-flex items-center', className)}>
            <button
                ref={triggerRef}
                id={triggerId}
                type="button"
                disabled={disabled}
                aria-label={ariaLabel}
                aria-haspopup="dialog"
                aria-expanded={open}
                aria-controls={open ? panelId : undefined}
                onClick={() => (open ? closeCalendar(false) : openCalendar())}
                onKeyDown={handleTriggerKeyDown}
                className={cn(
                    'inline-flex h-9 items-center gap-2 rounded-[9px] border bg-white px-3 text-[13px] leading-none transition-[border-color,box-shadow,background-color] duration-150',
                    hasValue && !disabled ? 'pr-8' : 'pr-3',
                    open
                        ? 'border-accent ring-[3px] ring-accent/15'
                        : hasValue
                          ? 'border-accent/45 bg-accent-tint/35 text-accent-2 hover:border-accent/70'
                          : 'border-ui-line text-ui-text hover:border-ui-line-strong hover:bg-ui-subtle/50',
                    disabled && 'cursor-not-allowed bg-ui-subtle text-ui-sub opacity-60',
                )}
            >
                <Calendar
                    aria-hidden="true"
                    className={cn(
                        'h-3.5 w-3.5 shrink-0 transition-colors',
                        open || hasValue ? 'text-accent' : 'text-ui-sub',
                    )}
                />

                {hasValue ? (
                    <span className="tnum inline-flex items-center gap-1.5 font-medium text-accent-2">
                        <span>{from || '不限'}</span>
                        <ArrowRight aria-hidden="true" className="h-3 w-3 text-accent/70" />
                        <span>{to || '至今'}</span>
                    </span>
                ) : (
                    <span className="inline-flex items-center gap-1.5 text-[#889b98]">
                        <span>{placeholder}</span>
                    </span>
                )}
            </button>

            {hasValue && !disabled ? (
                <button
                    type="button"
                    aria-label="清除日期范围"
                    onClick={handleClear}
                    className="absolute top-1/2 right-2 inline-flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded-full text-accent-2/70 transition-colors hover:bg-accent/15 hover:text-accent-2"
                >
                    <X className="h-3 w-3" />
                </button>
            ) : null}

            {open && position
                ? createPortal(
                      <dialog
                          ref={panelRef}
                          id={panelId}
                          open
                          aria-label={ariaLabel}
                          style={{
                              position: 'fixed',
                              top: position.top,
                              left: position.left,
                              right: 'auto',
                              transform: position.openUpward ? 'translateY(-100%)' : undefined,
                          }}
                          className={cn(
                              'animate-fade-in z-[70] m-0 max-w-[calc(100vw-16px)] overflow-hidden rounded-2xl border border-ui-line bg-white p-0 text-ui-text shadow-[0_18px_44px_-10px_rgba(17,25,23,0.18),0_4px_14px_-4px_rgba(17,25,23,0.06)]',
                              panelClassName,
                          )}
                      >
                          <div className="flex flex-col sm:flex-row">
                              {/* Quick presets */}
                              <div className="flex shrink-0 gap-1 overflow-x-auto border-b border-ui-line/80 bg-ui-subtle/55 p-2 sm:w-[116px] sm:flex-col sm:border-r sm:border-b-0 sm:p-2.5">
                                  <span className="hidden px-2 pt-0.5 pb-1 text-[11px] font-medium tracking-wider text-ui-sub/75 sm:block">
                                      快捷范围
                                  </span>
                                  {RANGE_PRESETS.map((preset) => {
                                      const target = preset.getRange(todayIso);
                                      const isActive =
                                          draftStart === null &&
                                          from === target.from &&
                                          to === target.to;
                                      return (
                                          <button
                                              key={preset.id}
                                              type="button"
                                              onClick={() => handlePresetClick(preset)}
                                              className={cn(
                                                  'shrink-0 rounded-lg px-2.5 py-1.5 text-left text-[12.5px] transition-colors',
                                                  isActive
                                                      ? 'bg-accent-tint font-medium text-accent-2'
                                                      : 'text-ui-sub hover:bg-white hover:text-ui-text',
                                              )}
                                          >
                                              {preset.label}
                                          </button>
                                      );
                                  })}
                              </div>

                              {/* Calendars + Footer */}
                              <div className="flex flex-col">
                                  <div className="flex flex-col gap-4 p-3.5 sm:flex-row sm:gap-5">
                                      {monthsToRender.map((ym, idx) =>
                                          renderMonthPanel(ym, idx, monthsToRender.length),
                                      )}
                                  </div>

                                  <div className="flex flex-wrap items-center justify-between gap-2 border-t border-ui-line/80 bg-ui-subtle/60 px-3.5 py-2.5 text-[12px]">
                                      {draftStart !== null ? (
                                          <div className="flex items-center gap-2 text-ui-sub">
                                              <span className="tnum font-medium text-ui-text">
                                                  {draftStart}
                                              </span>
                                              <span>起 · 请点击结束日期</span>
                                          </div>
                                      ) : hasValue ? (
                                          <div className="flex items-center gap-2 text-ui-sub">
                                              <span className="tnum font-medium text-ui-text">
                                                  {from || '不限'} 至 {to || '至今'}
                                              </span>
                                              {committedDays !== null && committedDays > 0 ? (
                                                  <span className="tnum rounded-full bg-accent-tint px-2 py-0.5 text-[11px] font-medium text-accent-2">
                                                      共 {committedDays} 天
                                                  </span>
                                              ) : null}
                                          </div>
                                      ) : (
                                          <span className="text-ui-sub">
                                              点击选择开始与结束日期
                                          </span>
                                      )}

                                      <div className="flex items-center gap-1.5">
                                          {draftStart !== null ? (
                                              <button
                                                  type="button"
                                                  onClick={() => {
                                                      onChange({
                                                          from: draftStart,
                                                          to: draftStart,
                                                      });
                                                      closeCalendar(true);
                                                  }}
                                                  className="rounded-md bg-accent-tint px-2.5 py-1 text-[12px] font-medium text-accent-2 transition-colors hover:bg-accent/15"
                                              >
                                                  仅选当天
                                              </button>
                                          ) : null}
                                          {hasValue ? (
                                              <button
                                                  type="button"
                                                  onClick={() => {
                                                      onChange({ from: '', to: '' });
                                                      closeCalendar(true);
                                                  }}
                                                  className="rounded-md px-2 py-1 text-[12px] font-medium text-ui-sub transition-colors hover:bg-ui-line/60 hover:text-ui-text"
                                              >
                                                  清除
                                              </button>
                                          ) : null}
                                      </div>
                                  </div>
                              </div>
                          </div>
                      </dialog>,
                      document.body,
                  )
                : null}
        </div>
    );
}
