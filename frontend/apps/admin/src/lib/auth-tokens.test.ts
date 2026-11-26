import { afterEach, describe, expect, it, vi } from 'vitest';
import { createAuthFetch } from './api';
import {
    clearTokens,
    getAccessToken,
    getRefreshToken,
    resetAuthTokenStateForTest,
    setTokenPair,
    subscribeAuthExpired,
} from './auth-tokens';

function jsonResponse(body: unknown, status: number): Response {
    return new Response(JSON.stringify(body), {
        status,
        headers: { 'Content-Type': 'application/json' },
    });
}

describe('auth-tokens & single-flight refresh', () => {
    afterEach(() => {
        resetAuthTokenStateForTest();
        localStorage.clear();
        sessionStorage.clear();
        vi.restoreAllMocks();
    });

    it('keeps access_token in memory only and refresh_token in sessionStorage only', () => {
        setTokenPair({
            access_token: 'access-1',
            refresh_token: 'refresh-1',
            access_token_expires_at: '2026-11-25T18:00:00+08:00',
            refresh_token_expires_at: '2026-12-02T18:00:00+08:00',
        });

        expect(getAccessToken()).toBe('access-1');
        expect(getRefreshToken()).toBe('refresh-1');
        expect(localStorage.length).toBe(0);
        expect(sessionStorage.getItem('silva_admin_refresh_token')).toBe('refresh-1');
        expect(Object.values(sessionStorage)).not.toContain('access-1');

        clearTokens();
        expect(getAccessToken()).toBeNull();
        expect(getRefreshToken()).toBeNull();
    });

    it('deduplicates concurrent 401 token_expired into one single-flight refresh and retries once', async () => {
        setTokenPair({
            access_token: 'expired-access',
            refresh_token: 'refresh-v1',
            access_token_expires_at: '2026-11-25T17:00:00+08:00',
            refresh_token_expires_at: '2026-12-02T17:00:00+08:00',
        });

        let refreshCalls = 0;
        const mockFetch = vi.fn(async (input: RequestInfo | URL) => {
            const req = input instanceof Request ? input : new Request(input);
            const url = new URL(req.url, 'http://localhost');

            if (url.pathname === '/api/v1/admin/tokens') {
                refreshCalls += 1;
                await new Promise((r) => setTimeout(r, 15));
                return jsonResponse(
                    {
                        data: {
                            access_token: 'fresh-access',
                            refresh_token: 'refresh-v2',
                            access_token_expires_at: '2026-11-25T18:15:00+08:00',
                            refresh_token_expires_at: '2026-12-02T18:00:00+08:00',
                        },
                    },
                    201,
                );
            }

            const auth = req.headers.get('Authorization');
            if (auth === 'Bearer expired-access') {
                return jsonResponse(
                    {
                        error: {
                            code: 'token_expired',
                            message: 'Access token expired',
                            details: [],
                        },
                    },
                    401,
                );
            }
            if (auth === 'Bearer fresh-access') {
                return jsonResponse({ data: { path: url.pathname } }, 200);
            }
            return jsonResponse(
                { error: { code: 'unauthorized', message: 'No auth', details: [] } },
                401,
            );
        });

        const wrappedFetch = createAuthFetch(mockFetch as unknown as typeof fetch);

        const [r1, r2, r3] = await Promise.all([
            wrappedFetch('/api/v1/admin/posts'),
            wrappedFetch('/api/v1/admin/categories'),
            wrappedFetch('/api/v1/admin/tags'),
        ]);

        expect(refreshCalls).toBe(1);
        expect(r1.status).toBe(200);
        expect(r2.status).toBe(200);
        expect(r3.status).toBe(200);
        expect(getAccessToken()).toBe('fresh-access');
        expect(getRefreshToken()).toBe('refresh-v2');
    });

    it('notifies expiration listeners and clears tokens when refresh fails', async () => {
        setTokenPair({
            access_token: 'expired-access',
            refresh_token: 'revoked-refresh',
            access_token_expires_at: '2026-11-25T17:00:00+08:00',
            refresh_token_expires_at: '2026-12-02T17:00:00+08:00',
        });

        const expiredSpy = vi.fn();
        const unsubscribe = subscribeAuthExpired(expiredSpy);

        const mockFetch = vi.fn(async (input: RequestInfo | URL) => {
            const req = input instanceof Request ? input : new Request(input);
            const url = new URL(req.url, 'http://localhost');
            if (url.pathname === '/api/v1/admin/tokens') {
                return jsonResponse(
                    { error: { code: 'unauthorized', message: 'Revoked', details: [] } },
                    401,
                );
            }
            return jsonResponse(
                { error: { code: 'token_expired', message: 'Expired', details: [] } },
                401,
            );
        });

        const wrappedFetch = createAuthFetch(mockFetch as unknown as typeof fetch);
        const res = await wrappedFetch('/api/v1/admin/users/me');

        expect(res.status).toBe(401);
        expect(expiredSpy).toHaveBeenCalledTimes(1);
        expect(getAccessToken()).toBeNull();
        expect(getRefreshToken()).toBeNull();

        unsubscribe();
    });
});
