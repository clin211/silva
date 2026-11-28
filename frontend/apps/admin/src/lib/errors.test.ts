import { describe, expect, it, vi } from 'vitest';
import { ApiError, applyApiFieldErrors, formatErrorWithRequestId } from './errors';

describe('errors mapping', () => {
    it('maps backend error codes to localized Chinese messages without leaking English serverMessage', () => {
        const err = new ApiError({
            status: 409,
            code: 'revision_conflict',
            message: 'expected_revision_id does not match current_revision_id',
        });

        expect(err.message).toContain('停止自动保存');
        expect(err.message).not.toContain('expected_revision_id');
    });

    it('includes Retry-After wait time for 429 rate_limited', () => {
        const err = new ApiError({
            status: 429,
            code: 'rate_limited',
            retryAfter: 900,
        });

        expect(err.message).toContain('15 分钟');
    });

    it('appends X-Request-Id on 5xx errors', () => {
        const err = new ApiError({
            status: 500,
            code: 'internal_error',
            requestId: '019535d9-3df7-79fb-b466-fa907fa17f9e',
        });

        expect(formatErrorWithRequestId(err)).toContain('019535d9-3df7-79fb-b466-fa907fa17f9e');
    });

    it('maps 422 details onto React Hook Form setError', () => {
        const setError = vi.fn();
        const err = new ApiError({
            status: 422,
            code: 'validation_error',
            details: [
                { field: 'email', code: 'invalid_format', message: 'invalid email' },
                { field: '', code: 'invalid_type', message: 'body must be object' },
            ],
        });

        const unmatched = applyApiFieldErrors(err, setError);
        expect(setError).toHaveBeenCalledWith('email', {
            type: 'invalid_format',
            message: '格式不符合要求',
        });
        expect(unmatched).toHaveLength(1);
    });

    it('maps series conflict error codes to clear Chinese messages', () => {
        expect(new ApiError({ status: 409, code: 'series_edit_conflict' }).message).toContain(
            '最新版本',
        );
        expect(new ApiError({ status: 409, code: 'series_in_use' }).message).toContain('下线');
        expect(new ApiError({ status: 409, code: 'post_already_in_series' }).message).toContain(
            '其他系列',
        );
        expect(new ApiError({ status: 409, code: 'already_published' }).message).toContain(
            '公开状态',
        );
    });
});
