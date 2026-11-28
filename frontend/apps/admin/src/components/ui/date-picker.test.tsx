import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
    DatePicker,
    DateRangePicker,
    formatIsoDate,
    getTodayIso,
    parseIsoDate,
} from '@/components/ui/date-picker';

afterEach(() => {
    cleanup();
});

describe('date-picker helpers', () => {
    it('parses valid ISO dates including leap days and rejects invalid dates', () => {
        expect(parseIsoDate('2024-02-29')).toEqual({ year: 2024, month: 2, day: 29 });
        expect(parseIsoDate('2025-02-29')).toBeNull();
        expect(parseIsoDate('2026-13-01')).toBeNull();
        expect(parseIsoDate('')).toBeNull();
        expect(formatIsoDate({ year: 2026, month: 4, day: 7 })).toBe('2026-04-07');
    });
});

describe('DatePicker', () => {
    it('opens calendar popover, allows selecting a date, and clears value', async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();

        const { rerender } = render(
            <DatePicker
                value="2026-11-10"
                onChange={onChange}
                ariaLabel="开始日期"
                placeholder="选择开始日期"
            />,
        );

        const trigger = screen.getByRole('button', { name: '开始日期' });
        expect(trigger).toHaveTextContent('2026-11-10');

        await user.click(trigger);
        expect(screen.getByRole('dialog', { name: '开始日期' })).toBeInTheDocument();

        await user.click(screen.getByRole('button', { name: '2026-11-18' }));
        expect(onChange).toHaveBeenCalledWith('2026-11-18');
        expect(screen.queryByRole('dialog', { name: '开始日期' })).not.toBeInTheDocument();

        rerender(
            <DatePicker
                value="2026-11-18"
                onChange={onChange}
                ariaLabel="开始日期"
                placeholder="选择开始日期"
            />,
        );

        await user.click(screen.getByRole('button', { name: '清除日期' }));
        expect(onChange).toHaveBeenCalledWith('');
    });

    it('supports switching year and month views and respects min constraint', async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();

        render(
            <DatePicker
                value="2026-11-15"
                min="2026-11-10"
                onChange={onChange}
                ariaLabel="结束日期"
            />,
        );

        await user.click(screen.getByRole('button', { name: '结束日期' }));

        const disabledDay = screen.getByRole('button', { name: '2026-11-05' });
        expect(disabledDay).toBeDisabled();

        await user.click(screen.getByRole('button', { name: '2026年' }));
        await user.click(screen.getByRole('button', { name: '2027' }));
        await user.click(screen.getByRole('button', { name: '3月' }));
        await user.click(screen.getByRole('button', { name: '2027-03-12' }));

        expect(onChange).toHaveBeenCalledWith('2027-03-12');
    });

    it('selects today via footer shortcut and closes on Escape', async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();

        render(<DatePicker value="" onChange={onChange} ariaLabel="发布日期" />);

        await user.click(screen.getByRole('button', { name: '发布日期' }));
        await user.keyboard('{Escape}');
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

        await user.click(screen.getByRole('button', { name: '发布日期' }));
        await user.click(screen.getByRole('button', { name: '今天' }));
        expect(onChange).toHaveBeenCalledWith(getTodayIso());
    });
});

describe('DateRangePicker', () => {
    it('selects a date range across clicks and swaps reversed order automatically', async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();

        render(
            <DateRangePicker
                from="2026-11-01"
                to="2026-11-05"
                onChange={onChange}
                ariaLabel="审计日志日期范围"
            />,
        );

        await user.click(screen.getByRole('button', { name: '审计日志日期范围' }));
        expect(screen.getByRole('dialog', { name: '审计日志日期范围' })).toBeInTheDocument();

        // Click end date first, then earlier start date -> should normalize [from, to]
        await user.click(screen.getByRole('button', { name: '2026-11-20' }));
        expect(onChange).not.toHaveBeenCalled();

        await user.click(screen.getByRole('button', { name: '2026-11-12' }));
        expect(onChange).toHaveBeenCalledWith({ from: '2026-11-12', to: '2026-11-20' });
    });

    it('applies quick presets and supports clearing range', async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();
        const today = getTodayIso();

        const { rerender } = render(
            <DateRangePicker from="" to="" onChange={onChange} ariaLabel="审计日志日期范围" />,
        );

        await user.click(screen.getByRole('button', { name: '审计日志日期范围' }));
        await user.click(screen.getByRole('button', { name: '今天' }));
        expect(onChange).toHaveBeenCalledWith({ from: today, to: today });

        rerender(
            <DateRangePicker
                from={today}
                to={today}
                onChange={onChange}
                ariaLabel="审计日志日期范围"
            />,
        );

        await user.click(screen.getByRole('button', { name: '清除日期范围' }));
        expect(onChange).toHaveBeenCalledWith({ from: '', to: '' });
    });

    it('supports picking a single day via footer shortcut after first click', async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();

        render(
            <DateRangePicker
                from="2026-11-01"
                to="2026-11-03"
                onChange={onChange}
                ariaLabel="审计日志日期范围"
            />,
        );

        await user.click(screen.getByRole('button', { name: '审计日志日期范围' }));
        await user.click(screen.getByRole('button', { name: '2026-11-16' }));
        await user.click(screen.getByRole('button', { name: '仅选当天' }));

        expect(onChange).toHaveBeenCalledWith({ from: '2026-11-16', to: '2026-11-16' });
    });
});
