const numberFormatter = new Intl.NumberFormat('zh-CN');

export function formatNumber(value: number): string {
    return numberFormatter.format(value);
}

/**
 * 格式化服务端返回的 +08:00 RFC 3339 时间字符串。
 * 保留服务端北京时间语义，避免在非东八区设备上漂移。
 */
export function formatDateTime(iso: string | null | undefined): string {
    if (!iso) return '—';
    const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?/.exec(iso);
    if (!match) return iso;
    const [, year, month, day, hour, minute, second] = match;
    return second
        ? `${year}-${month}-${day} ${hour}:${minute}:${second}`
        : `${year}-${month}-${day} ${hour}:${minute}`;
}

export function formatShortDateTime(iso: string | null | undefined): string {
    if (!iso) return '—';
    const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(iso);
    if (!match) return iso;
    const [, , month, day, hour, minute] = match;
    return `${month}-${day} ${hour}:${minute}`;
}

export function formatRelativeTime(iso: string | null | undefined): string {
    if (!iso) return '—';
    const ms = Date.parse(iso);
    if (Number.isNaN(ms)) return iso;
    const diffSec = Math.round((Date.now() - ms) / 1000);
    if (diffSec < 60) return '刚刚';
    if (diffSec < 3600) return `${Math.floor(diffSec / 60)} 分钟前`;
    if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} 小时前`;
    if (diffSec < 86400 * 7) return `${Math.floor(diffSec / 86400)} 天前`;
    return formatShortDateTime(iso);
}

export function formatDurationSeconds(seconds: number): string {
    if (seconds < 0 || !Number.isFinite(seconds)) return '—';
    const total = Math.floor(seconds);
    const hrs = Math.floor(total / 3600);
    const mins = Math.floor((total % 3600) / 60);
    const secs = total % 60;
    if (hrs > 0) return `${hrs} 小时 ${mins} 分 ${secs} 秒`;
    if (mins > 0) return `${mins} 分 ${secs} 秒`;
    return `${secs} 秒`;
}

export function formatElapsed(startIso: string | null | undefined, endIso?: string | null): string {
    if (!startIso) return '—';
    const startMs = Date.parse(startIso);
    if (Number.isNaN(startMs)) return '—';
    const endMs = endIso ? Date.parse(endIso) : Date.now();
    if (Number.isNaN(endMs)) return '—';
    return formatDurationSeconds(Math.max(0, (endMs - startMs) / 1000));
}

export function isDelayedOver5Minutes(createdAt: string): boolean {
    const ms = Date.parse(createdAt);
    if (Number.isNaN(ms)) return false;
    return Date.now() - ms > 5 * 60 * 1000;
}

export function formatBytes(bytes: number | null | undefined): string {
    if (bytes === null || bytes === undefined) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function formatGeneration(generation: number | null | undefined): string {
    if (generation === null || generation === undefined) return '—';
    return `g-${String(generation).padStart(4, '0')}`;
}

export function shortUuid(id: string | null | undefined): string {
    if (!id) return '—';
    if (id.length <= 16) return id;
    return `${id.slice(0, 8)}…${id.slice(-4)}`;
}

export function utf8ByteLength(value: string): number {
    return new TextEncoder().encode(value).length;
}
