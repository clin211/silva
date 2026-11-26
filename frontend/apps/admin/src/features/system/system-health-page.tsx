import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Activity, Gauge, Globe, RefreshCw, Search, Terminal } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Button } from '@/components/ui/button';
import { PageContainer, PageHeader } from '@/components/ui/feedback';
import { StatusChip } from '@/components/ui/status-chip';
import { apiClient, fetchHealthLive, fetchMetricsText, unwrapResponse } from '@/lib/api';
import { formatDateTime, formatGeneration } from '@/lib/format';
import { queryKeys } from '@/lib/query-keys';

interface ParsedMetric {
    name: string;
    labels: string;
    value: string;
}

function parsePrometheusSample(text: string): {
    businessMetricsUp: number | null;
    silvaMetrics: ParsedMetric[];
} {
    let businessMetricsUp: number | null = null;
    const silvaMetrics: ParsedMetric[] = [];

    for (const rawLine of text.split('\n')) {
        const line = rawLine.trim();
        if (!line || line.startsWith('#')) continue;
        const match = /^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{[^}]*\})?\s+([^\s]+)/.exec(line);
        if (!match) continue;
        const [, name, labels = '', value] = match;
        if (!name || !value) continue;
        if (name === 'silva_business_metrics_up') {
            const parsed = Number.parseFloat(value);
            businessMetricsUp = Number.isFinite(parsed) ? parsed : null;
        }
        if (name.startsWith('silva_')) {
            silvaMetrics.push({ name, labels, value });
        }
    }

    return { businessMetricsUp, silvaMetrics };
}

export function SystemHealthPage() {
    const [showRawMetrics, setShowRawMetrics] = useState(false);
    const [metricSearch, setMetricSearch] = useState('');

    const liveQuery = useQuery({
        queryKey: queryKeys.system.health,
        queryFn: fetchHealthLive,
        refetchInterval: 60_000,
        retry: false,
    });

    const metricsQuery = useQuery({
        queryKey: queryKeys.system.metrics,
        queryFn: fetchMetricsText,
        refetchInterval: 60_000,
        retry: false,
    });

    const siteQuery = useQuery({
        queryKey: queryKeys.publicSite.current,
        queryFn: async () => {
            const res = await unwrapResponse(apiClient.GET('/api/v1/public/site'));
            return res.data;
        },
        refetchInterval: 60_000,
        retry: false,
    });

    const parsedMetrics = useMemo(
        () => parsePrometheusSample(metricsQuery.data?.text ?? ''),
        [metricsQuery.data?.text],
    );

    const filteredMetrics = useMemo(() => {
        const q = metricSearch.trim().toLowerCase();
        if (!q) return parsedMetrics.silvaMetrics;
        return parsedMetrics.silvaMetrics.filter(
            (m) => m.name.toLowerCase().includes(q) || m.labels.toLowerCase().includes(q),
        );
    }, [parsedMetrics.silvaMetrics, metricSearch]);

    const handleRefresh = () => {
        void liveQuery.refetch();
        void metricsQuery.refetch();
        void siteQuery.refetch();
    };

    const allHealthy =
        liveQuery.data?.status === 'ok' &&
        !metricsQuery.isError &&
        parsedMetrics.businessMetricsUp !== 0;

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="OBSERVABILITY"
                title="系统健康"
                description="实时检测 /health/live 存活探针、/metrics 业务采样指标与公开站点快照状态，每分钟自动刷新。"
                actions={
                    <Button
                        variant="secondary"
                        size="sm"
                        onClick={handleRefresh}
                        disabled={liveQuery.isFetching || metricsQuery.isFetching}
                    >
                        <RefreshCw
                            className={
                                liveQuery.isFetching || metricsQuery.isFetching
                                    ? 'h-3.5 w-3.5 animate-spin'
                                    : 'h-3.5 w-3.5'
                            }
                        />
                        立即刷新
                    </Button>
                }
            />

            {/* 顶部三栏健康卡片 */}
            <div className="grid gap-4 md:grid-cols-3">
                <section className="studio-card p-5">
                    <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-ui-sub">API 进程存活</span>
                        <span className="grid h-8 w-8 place-items-center rounded-lg bg-ok-tint text-ok">
                            <Activity className="h-4 w-4" />
                        </span>
                    </div>
                    <div className="mt-3 flex items-center justify-between">
                        <span className="tnum font-code text-[22px] font-semibold text-ui-text">
                            {liveQuery.data ? `${liveQuery.data.latencyMs} ms` : '—'}
                        </span>
                        {liveQuery.isLoading ? (
                            <StatusChip tone="mute">检查中</StatusChip>
                        ) : liveQuery.isError ? (
                            <StatusChip tone="bad">不可达</StatusChip>
                        ) : (
                            <StatusChip tone="ok">正常</StatusChip>
                        )}
                    </div>
                    <p className="mt-2 font-code text-[11.5px] text-ui-sub">
                        GET /health/live ·{' '}
                        {liveQuery.data ? formatDateTime(liveQuery.data.checkedAt) : '—'}
                    </p>
                </section>

                <section className="studio-card p-5">
                    <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-ui-sub">Prometheus 采样</span>
                        <span className="grid h-8 w-8 place-items-center rounded-lg bg-accent-tint text-accent">
                            <Gauge className="h-4 w-4" />
                        </span>
                    </div>
                    <div className="mt-3 flex items-center justify-between">
                        <span className="tnum font-code text-[22px] font-semibold text-ui-text">
                            {metricsQuery.isError
                                ? '受限'
                                : `${parsedMetrics.silvaMetrics.length} 条`}
                        </span>
                        {metricsQuery.isLoading ? (
                            <StatusChip tone="mute">检查中</StatusChip>
                        ) : metricsQuery.isError ? (
                            <StatusChip tone="warn">未开放</StatusChip>
                        ) : parsedMetrics.businessMetricsUp === 0 ? (
                            <StatusChip tone="bad">采样异常</StatusChip>
                        ) : (
                            <StatusChip tone="ok">正常</StatusChip>
                        )}
                    </div>
                    <p className="mt-2 font-code text-[11.5px] text-ui-sub">
                        silva_business_metrics_up={parsedMetrics.businessMetricsUp ?? '—'}
                    </p>
                </section>

                <section className="studio-card p-5">
                    <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-ui-sub">公开站点快照</span>
                        <span className="grid h-8 w-8 place-items-center rounded-lg bg-ui-bg text-ui-sub">
                            <Globe className="h-4 w-4" />
                        </span>
                    </div>
                    <div className="mt-3 flex items-center justify-between">
                        <span className="tnum font-code text-[22px] font-semibold text-ui-text">
                            {siteQuery.data
                                ? formatGeneration(siteQuery.data.generation)
                                : '未上线'}
                        </span>
                        {siteQuery.isLoading ? (
                            <StatusChip tone="mute">检查中</StatusChip>
                        ) : siteQuery.data ? (
                            <StatusChip tone="ok">已上线</StatusChip>
                        ) : (
                            <StatusChip tone="warn">尚未上线</StatusChip>
                        )}
                    </div>
                    <p className="mt-2 text-[12px] text-ui-sub">
                        {siteQuery.data ? (
                            <>生成于 {formatDateTime(siteQuery.data.generated_at)}</>
                        ) : (
                            <Link
                                to="/publishing"
                                className="font-medium text-accent hover:underline"
                            >
                                查看发布任务队列 →
                            </Link>
                        )}
                    </p>
                </section>
            </div>

            {/* 全局状态条 */}
            {liveQuery.isError ? (
                <div
                    role="alert"
                    className="rounded-xl border border-bad/25 bg-bad-tint px-4 py-3 text-[13.5px] text-bad"
                >
                    无法访问 API 存活探针（/health/live），请检查后端进程或代理配置。
                </div>
            ) : !allHealthy ? (
                <div
                    role="alert"
                    className="rounded-xl border border-warn/25 bg-warn-tint px-4 py-3 text-[13.5px] text-warn"
                >
                    部分监控端点需要关注（如 /metrics 访问受限或业务采样尚未就绪）。
                </div>
            ) : null}

            {/* Silva Prometheus 指标检索卡片 */}
            {parsedMetrics.silvaMetrics.length > 0 ? (
                <section className="studio-card overflow-hidden">
                    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ui-line bg-ui-subtle/70 px-5 py-3.5">
                        <div>
                            <h2 className="text-[15px] font-semibold text-ui-text">
                                Silva Prometheus 指标明细
                            </h2>
                            <p className="mt-0.5 text-[12px] text-ui-sub">
                                共 {parsedMetrics.silvaMetrics.length} 条 silva_* 采样条目，当前展示{' '}
                                {Math.min(80, filteredMetrics.length)} 条。
                            </p>
                        </div>
                        <div className="flex flex-wrap items-center gap-2">
                            <div className="relative">
                                <Search
                                    aria-hidden="true"
                                    className="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[#889B98]"
                                />
                                <input
                                    type="search"
                                    value={metricSearch}
                                    onChange={(event) => setMetricSearch(event.target.value)}
                                    placeholder="过滤指标名或标签…"
                                    className="studio-input h-8 w-56 pl-8 text-[12.5px]"
                                />
                            </div>
                            <Button
                                variant="secondary"
                                size="sm"
                                onClick={() => setShowRawMetrics((prev) => !prev)}
                            >
                                <Terminal className="h-3.5 w-3.5" />
                                {showRawMetrics ? '隐藏原始文本' : '原始 /metrics'}
                            </Button>
                        </div>
                    </div>

                    <div className="max-h-[400px] overflow-auto">
                        <table className="tbl min-w-[600px]">
                            <thead className="sticky top-0 z-10">
                                <tr>
                                    <th>指标名</th>
                                    <th>标签</th>
                                    <th className="text-right">当前值</th>
                                </tr>
                            </thead>
                            <tbody>
                                {filteredMetrics.slice(0, 80).map((m, idx) => (
                                    <tr key={`${m.name}-${m.labels}-${idx}`}>
                                        <td className="font-code text-[12px] font-medium text-ui-text">
                                            {m.name}
                                        </td>
                                        <td className="max-w-[420px] truncate font-code text-[11.5px] text-ui-sub">
                                            {m.labels || '—'}
                                        </td>
                                        <td className="tnum text-right font-code text-[12.5px] font-medium text-accent-2">
                                            {m.value}
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>

                    {showRawMetrics && metricsQuery.data?.text ? (
                        <div className="border-t border-ui-line p-4">
                            <pre className="max-h-96 overflow-auto rounded-xl bg-[#091C1A] p-4 font-code text-[12px] leading-5 text-[#D7EEEA]">
                                {metricsQuery.data.text}
                            </pre>
                        </div>
                    ) : null}
                </section>
            ) : null}
        </PageContainer>
    );
}
