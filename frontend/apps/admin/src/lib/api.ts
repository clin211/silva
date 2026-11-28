import { createSilvaClient, type Schemas } from '@silva/api-client';
import { getAccessToken, notifyAuthExpired, refreshTokensSingleFlight } from './auth-tokens';
import { ApiError } from './errors';

const PUBLIC_OR_UNAUTHENTICATED_PATHS = new Set([
    '/api/v1/admin/sessions',
    '/api/v1/admin/tokens',
    '/health/live',
    '/metrics',
]);

function extractPathname(input: RequestInfo | URL): string {
    if (typeof input === 'string') {
        try {
            return new URL(input, 'http://localhost').pathname;
        } catch {
            return input;
        }
    }
    if (input instanceof URL) {
        return input.pathname;
    }
    try {
        return new URL(input.url, 'http://localhost').pathname;
    } catch {
        return input.url;
    }
}

function shouldAttachBearer(pathname: string): boolean {
    if (PUBLIC_OR_UNAUTHENTICATED_PATHS.has(pathname)) return false;
    if (pathname.startsWith('/api/v1/public/')) return false;
    return pathname.startsWith('/api/v1/admin/');
}

function parseRetryAfter(response: Response): number | null {
    const raw = response.headers.get('Retry-After');
    if (!raw) return null;
    const parsed = Number.parseInt(raw, 10);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

async function parseErrorEnvelope(
    response: Response,
): Promise<Schemas['ErrorResponse']['error'] | null> {
    const contentType = response.headers.get('Content-Type') ?? '';
    if (!contentType.includes('json')) return null;
    try {
        const json = (await response.clone().json()) as Partial<Schemas['ErrorResponse']>;
        return json.error ?? null;
    } catch {
        return null;
    }
}

export async function responseToApiError(response: Response): Promise<ApiError> {
    const envelope = await parseErrorEnvelope(response);
    return new ApiError({
        status: response.status,
        code: envelope?.code,
        message: envelope?.message,
        details: envelope?.details,
        mediaUsage: envelope?.media_usage,
        requestId: response.headers.get('X-Request-Id'),
        traceId: response.headers.get('X-Trace-Id'),
        retryAfter: parseRetryAfter(response),
    });
}

function withBearer(request: Request, token: string | null): Request {
    const headers = new Headers(request.headers);
    if (token) {
        headers.set('Authorization', `Bearer ${token}`);
    } else {
        headers.delete('Authorization');
    }
    return new Request(request, { headers });
}

function resolveRequestInput(input: RequestInfo | URL): RequestInfo | URL {
    if (typeof input === 'string' && input.startsWith('/')) {
        const origin =
            typeof globalThis.location !== 'undefined' &&
            globalThis.location.origin &&
            globalThis.location.origin !== 'null'
                ? globalThis.location.origin
                : 'http://localhost';
        return new URL(input, origin).toString();
    }
    return input;
}

/**
 * 带自动 Bearer 注入与 401 token_expired 单飞刷新重放一次的底层 fetch。
 */
export function createAuthFetch(baseFetch: typeof fetch = globalThis.fetch): typeof fetch {
    return async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
        const baseRequest = new Request(resolveRequestInput(input), init);
        const pathname = extractPathname(baseRequest);
        const attach = shouldAttachBearer(pathname);

        // 保留一份可用于重试的克隆请求（在发送前克隆，确保 body 未被消费）
        const retryTemplate = attach ? baseRequest.clone() : null;
        const firstRequest = attach ? withBearer(baseRequest, getAccessToken()) : baseRequest;

        const response = await baseFetch(firstRequest);

        if (!attach || response.status !== 401) {
            return response;
        }

        const errorBody = await parseErrorEnvelope(response);
        if (errorBody?.code === 'token_expired') {
            const freshToken = await refreshTokensSingleFlight(baseFetch);
            if (freshToken && retryTemplate) {
                const retryResponse = await baseFetch(withBearer(retryTemplate, freshToken));
                if (retryResponse.status === 401) {
                    const retryError = await parseErrorEnvelope(retryResponse);
                    if (
                        retryError?.code === 'unauthorized' ||
                        retryError?.code === 'token_expired'
                    ) {
                        notifyAuthExpired('unauthorized');
                    }
                }
                return retryResponse;
            }
            return response;
        }

        if (errorBody?.code === 'unauthorized' && pathname !== '/api/v1/admin/sessions/current') {
            notifyAuthExpired('unauthorized');
        }

        return response;
    };
}

export const authFetch = createAuthFetch();

export const apiClient = createSilvaClient({
    baseUrl: '',
    fetch: authFetch,
});

interface OpenApiFetchResult<TData> {
    data?: TData;
    error?: unknown;
    response: Response;
}

/**
 * 统一解包 openapi-fetch 响应：
 * - 非 2xx 或存在 error 时抛出带 X-Request-Id 与稳定 error.code 的 ApiError。
 * - 204 返回 undefined。
 */
export async function unwrapResponse<TData>(
    promise: Promise<OpenApiFetchResult<TData>>,
): Promise<TData> {
    let result: OpenApiFetchResult<TData>;
    try {
        result = await promise;
    } catch {
        throw new ApiError({ status: 0, code: 'network_error' });
    }

    if (!result.response.ok || result.error !== undefined) {
        const errorEnvelope =
            typeof result.error === 'object' && result.error !== null && 'error' in result.error
                ? (result.error as Schemas['ErrorResponse']).error
                : null;

        if (errorEnvelope) {
            throw new ApiError({
                status: result.response.status,
                code: errorEnvelope.code,
                message: errorEnvelope.message,
                details: errorEnvelope.details,
                mediaUsage: errorEnvelope.media_usage,
                requestId: result.response.headers.get('X-Request-Id'),
                traceId: result.response.headers.get('X-Trace-Id'),
                retryAfter: parseRetryAfter(result.response),
            });
        }
        throw await responseToApiError(result.response);
    }

    return result.data as TData;
}

/**
 * 流式上传图片：POST /api/v1/admin/media?filename=...
 * 发送原始文件字节，由服务端嗅探格式、尺寸并校验。
 */
export async function uploadMediaFile(file: File): Promise<Schemas['MediaUpload']> {
    const query = new URLSearchParams({ filename: file.name });
    let response: Response;
    try {
        response = await authFetch(`/api/v1/admin/media?${query.toString()}`, {
            method: 'POST',
            headers: {
                'Content-Type': file.type || 'application/octet-stream',
                Accept: 'application/json',
            },
            body: file,
        });
    } catch {
        throw new ApiError({ status: 0, code: 'network_error' });
    }

    if (!response.ok) {
        throw await responseToApiError(response);
    }

    const payload = (await response.json()) as Schemas['MediaUploadResponse'];
    return payload.data;
}

/**
 * 通过 Bearer Token 读取已校验的私有媒体二进制内容。
 */
export async function fetchMediaBlob(mediaId: string, signal?: AbortSignal): Promise<Blob> {
    let response: Response;
    try {
        response = await authFetch(`/api/v1/admin/media/${encodeURIComponent(mediaId)}/content`, {
            method: 'GET',
            signal,
        });
    } catch (error) {
        if (error instanceof DOMException && error.name === 'AbortError') {
            throw error;
        }
        throw new ApiError({ status: 0, code: 'network_error' });
    }

    if (!response.ok) {
        throw await responseToApiError(response);
    }

    return response.blob();
}

export async function fetchHealthLive(): Promise<{
    status: string;
    latencyMs: number;
    checkedAt: string;
}> {
    const start = performance.now();
    let response: Response;
    try {
        response = await authFetch('/health/live', {
            method: 'GET',
            headers: { Accept: 'application/json' },
        });
    } catch {
        throw new ApiError({ status: 0, code: 'network_error' });
    }

    const latencyMs = Math.max(1, Math.round(performance.now() - start));
    if (!response.ok) {
        throw await responseToApiError(response);
    }
    const payload = (await response.json()) as Schemas['Liveness'];
    return {
        status: payload.status,
        latencyMs,
        checkedAt: new Date().toISOString(),
    };
}

export async function fetchMetricsText(): Promise<{ text: string; checkedAt: string }> {
    let response: Response;
    try {
        response = await authFetch('/metrics', {
            method: 'GET',
            headers: { Accept: 'text/plain' },
        });
    } catch {
        throw new ApiError({ status: 0, code: 'network_error' });
    }
    if (!response.ok) {
        throw await responseToApiError(response);
    }
    return {
        text: await response.text(),
        checkedAt: new Date().toISOString(),
    };
}
