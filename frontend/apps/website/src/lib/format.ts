const numberFormat = new Intl.NumberFormat('zh-CN');
const monthFormat = new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: 'long',
    timeZone: 'UTC',
});

/** RFC 3339 时间或日期的前 10 位（站点时区的日期）。 */
export const day = (value: string) => value.slice(0, 10);
export const number = (value: number) => numberFormat.format(value);
export const monthLabel = (key: string) => monthFormat.format(new Date(`${key}-01T00:00:00Z`));
export const growth = (value: number | null) =>
    value === null ? '数据不足' : `${value > 0 ? '+' : ''}${number(value)}`;
export const growthClass = (value: number | null) =>
    value === null || value === 0 ? 'muted' : value > 0 ? 'up' : 'down';
