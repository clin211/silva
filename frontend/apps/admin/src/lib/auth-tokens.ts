import type { Schemas } from '@silva/api-client';

const REFRESH_STORAGE_KEY = 'silva_admin_refresh_token';

/**
 * 安全边界（见 docs/02-frontend-technical.md §6）：
 * - access token 仅保存在内存中，绝不写入 localStorage / sessionStorage / Cookie。
 * - refresh token 仅保存在 sessionStorage，用于刷新页面后恢复会话，关闭标签页即丢弃。
 */
let memoryAccessToken: string | null = null;
let memoryAccessExpiresAt: string | null = null;
let inFlightRefresh: Promise<string | null> | null = null;

type ExpiredListener = (reason: 'expired' | 'unauthorized') => void;
const expiredListeners = new Set<ExpiredListener>();

function safeSessionStorage(): Storage | null {
    try {
        return globalThis.sessionStorage;
    } catch {
        return null;
    }
}

export function getAccessToken(): string | null {
    return memoryAccessToken;
}

export function getAccessTokenExpiresAt(): string | null {
    return memoryAccessExpiresAt;
}

export function getRefreshToken(): string | null {
    return safeSessionStorage()?.getItem(REFRESH_STORAGE_KEY) ?? null;
}

export function setTokenPair(pair: Schemas['TokenPair']): void {
    memoryAccessToken = pair.access_token;
    memoryAccessExpiresAt = pair.access_token_expires_at;
    safeSessionStorage()?.setItem(REFRESH_STORAGE_KEY, pair.refresh_token);
}

export function clearTokens(): void {
    memoryAccessToken = null;
    memoryAccessExpiresAt = null;
    safeSessionStorage()?.removeItem(REFRESH_STORAGE_KEY);
}

export function subscribeAuthExpired(listener: ExpiredListener): () => void {
    expiredListeners.add(listener);
    return () => {
        expiredListeners.delete(listener);
    };
}

export function notifyAuthExpired(reason: 'expired' | 'unauthorized'): void {
    clearTokens();
    for (const listener of expiredListeners) {
        listener(reason);
    }
}

/**
 * 单飞刷新：同一页面内并发遇到 token_expired 时共享同一次 POST /api/v1/admin/tokens 请求，
 * 避免重复提交已轮换的 refresh token 触发服务端重放检测并撤销整个会话。
 */
export function refreshTokensSingleFlight(
    fetchImpl: typeof fetch = globalThis.fetch,
): Promise<string | null> {
    if (inFlightRefresh) {
        return inFlightRefresh;
    }

    const refreshToken = getRefreshToken();
    if (!refreshToken) {
        return Promise.resolve(null);
    }

    inFlightRefresh = (async () => {
        try {
            const origin =
                typeof globalThis.location !== 'undefined' &&
                globalThis.location.origin &&
                globalThis.location.origin !== 'null'
                    ? globalThis.location.origin
                    : 'http://localhost';
            const refreshUrl = new URL('/api/v1/admin/tokens', origin).toString();
            const response = await fetchImpl(refreshUrl, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    Accept: 'application/json',
                },
                body: JSON.stringify({ refresh_token: refreshToken }),
            });

            if (!response.ok) {
                notifyAuthExpired('expired');
                return null;
            }

            const payload = (await response.json()) as { data?: Schemas['TokenPair'] };
            if (!payload.data?.access_token || !payload.data?.refresh_token) {
                notifyAuthExpired('expired');
                return null;
            }

            setTokenPair(payload.data);
            return payload.data.access_token;
        } catch {
            // 网络故障时不立即清空 refresh token，允许用户重试；如果是真正会话失效服务端会返回 401
            return null;
        } finally {
            inFlightRefresh = null;
        }
    })();

    return inFlightRefresh;
}

/** 仅供测试重置单飞状态 */
export function resetAuthTokenStateForTest(): void {
    clearTokens();
    inFlightRefresh = null;
}
