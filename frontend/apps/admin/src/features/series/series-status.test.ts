import { describe, expect, it } from 'vitest';
import {
    canDeleteSeries,
    completionStatusMeta,
    isValidSeriesSlug,
    seriesPublishStatusMeta,
} from './series-status';

describe('series status helpers', () => {
    it('distinguishes private, empty public and visible series', () => {
        expect(seriesPublishStatusMeta({ published_at: null, is_live: false }).label).toBe(
            '未公开',
        );
        const published_at = '2026-11-26T10:00:00+08:00';
        expect(seriesPublishStatusMeta({ published_at, is_live: false }).label).toBe(
            '暂无可公开文章',
        );
        expect(seriesPublishStatusMeta({ published_at, is_live: true }).label).toBe('已上线');
        expect(canDeleteSeries({ published_at: null })).toBe(true);
        expect(canDeleteSeries({ published_at })).toBe(false);
    });

    it('validates slugs and formats completion labels', () => {
        expect(isValidSeriesSlug('rust-concurrency-101')).toBe(true);
        expect(isValidSeriesSlug('Rust')).toBe(false);
        expect(isValidSeriesSlug('-bad')).toBe(false);
        expect(isValidSeriesSlug('bad--slug')).toBe(false);
        expect(completionStatusMeta('ongoing').label).toBe('连载中');
        expect(completionStatusMeta('completed').label).toBe('已完结');
    });
});
