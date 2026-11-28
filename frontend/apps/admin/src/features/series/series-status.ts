import type { Schemas } from '@silva/api-client';
import type { ChipTone } from '@/components/ui/status-chip';

export type SeriesStatusInput = Pick<Schemas['SeriesSummary'], 'published_at' | 'is_live'>;

export interface SeriesPublishStatusMeta {
    label: string;
    tone: ChipTone;
    description: string;
}

const SLUG_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

export function isValidSeriesSlug(value: string): boolean {
    const trimmed = value.trim();
    return trimmed.length > 0 && trimmed.length <= 100 && SLUG_PATTERN.test(trimmed);
}

export function seriesPublishStatusMeta(series: SeriesStatusInput): SeriesPublishStatusMeta {
    if (!series.published_at) return { label: '未公开', tone: 'mute', description: '仅后台可见。' };
    if (!series.is_live)
        return {
            label: '暂无可公开文章',
            tone: 'warn',
            description: '已设为公开。首篇成员文章上线时，专栏页会一同公开。',
        };
    return { label: '已上线', tone: 'ok', description: '访客可以查看专栏页和已发布的成员文章。' };
}

export function completionStatusMeta(status: Schemas['SeriesSummary']['completion_status']): {
    label: string;
    tone: ChipTone;
} {
    if (status === 'completed') {
        return { label: '已完结', tone: 'ok' };
    }
    return { label: '连载中', tone: 'info' };
}

export function canDeleteSeries(series: Pick<Schemas['SeriesSummary'], 'published_at'>): boolean {
    return series.published_at === null;
}
