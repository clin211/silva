import type { Schemas } from '@silva/api-client';
import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { DateRangePicker } from '@/components/ui/date-picker';
import {
    EmptyState,
    ErrorBanner,
    LoadingCard,
    PageContainer,
    PageHeader,
    PaginationBar,
} from '@/components/ui/feedback';
import { Drawer } from '@/components/ui/overlay';
import { apiClient, unwrapResponse } from '@/lib/api';
import { formatDateTime, formatShortDateTime, shortUuid } from '@/lib/format';
import { queryKeys } from '@/lib/query-keys';

export function AuditLogsPage() {
    const [page, setPage] = useState(1);
    const [fromDate, setFromDate] = useState('');
    const [toDate, setToDate] = useState('');
    const [actionFilter, setActionFilter] = useState('');
    const [resourceTypeFilter, setResourceTypeFilter] = useState('');
    const [requestIdInput, setRequestIdInput] = useState('');
    const [requestIdFilter, setRequestIdFilter] = useState('');
    const [selectedLog, setSelectedLog] = useState<Schemas['AuditLog'] | null>(null);

    const queryParams = {
        page,
        per_page: 20,
        from: fromDate ? `${fromDate}T00:00:00+08:00` : undefined,
        to: toDate ? `${toDate}T23:59:59+08:00` : undefined,
        action: actionFilter.trim() || undefined,
        resource_type: resourceTypeFilter.trim() || undefined,
        request_id: requestIdFilter.trim() || undefined,
    };

    const logsQuery = useQuery({
        queryKey: queryKeys.auditLogs.list(queryParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/audit-logs', {
                    params: { query: queryParams },
                }),
            ),
    });

    const detailQuery = useQuery({
        queryKey: queryKeys.auditLogs.detail(selectedLog?.id ?? ''),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/audit-logs/{id}', {
                    params: { path: { id: selectedLog!.id } },
                }),
            );
            return res.data;
        },
        enabled: Boolean(selectedLog?.id),
    });

    const logs = logsQuery.data?.data ?? [];
    const pagination = logsQuery.data?.meta.pagination;
    const hasFilters = Boolean(
        fromDate || toDate || actionFilter || resourceTypeFilter || requestIdFilter,
    );
    const detail = detailQuery.data ?? selectedLog;

    const resetFilters = () => {
        setFromDate('');
        setToDate('');
        setActionFilter('');
        setResourceTypeFilter('');
        setRequestIdInput('');
        setRequestIdFilter('');
        setPage(1);
    };

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="SECURITY AUDIT"
                title="审计日志"
                description="记录登录、账户、内容、媒体、分类标签、个人资料等关键写操作。日志只读。"
            />

            <section className="studio-card overflow-hidden">
                <form
                    onSubmit={(event) => {
                        event.preventDefault();
                        setRequestIdFilter(requestIdInput.trim());
                        setPage(1);
                    }}
                    className="flex flex-wrap items-center gap-2.5 border-b border-ui-line bg-ui-subtle/60 px-4 py-3 sm:px-5"
                >
                    <DateRangePicker
                        from={fromDate}
                        to={toDate}
                        onChange={({ from, to }) => {
                            setFromDate(from);
                            setToDate(to);
                            setPage(1);
                        }}
                        placeholder="按日期范围筛选"
                        ariaLabel="审计日志日期范围"
                        className="w-full sm:w-auto"
                    />

                    <label className="sr-only" htmlFor="audit-action">
                        事件类型
                    </label>
                    <input
                        id="audit-action"
                        type="text"
                        value={actionFilter}
                        onChange={(event) => {
                            setActionFilter(event.target.value);
                            setPage(1);
                        }}
                        placeholder="事件（如 auth.login）"
                        className="studio-input w-44 font-code text-[12.5px]"
                    />

                    <label className="sr-only" htmlFor="audit-resource">
                        资源类型
                    </label>
                    <input
                        id="audit-resource"
                        type="text"
                        value={resourceTypeFilter}
                        onChange={(event) => {
                            setResourceTypeFilter(event.target.value);
                            setPage(1);
                        }}
                        placeholder="资源类型（如 post）"
                        className="studio-input w-40 font-code text-[12.5px]"
                    />

                    <div className="relative w-full sm:w-60">
                        <label className="sr-only" htmlFor="audit-req-id">
                            Request ID
                        </label>
                        <Search className="pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-ui-sub" />
                        <input
                            id="audit-req-id"
                            type="search"
                            value={requestIdInput}
                            onChange={(event) => setRequestIdInput(event.target.value)}
                            onBlur={() => {
                                if (requestIdInput.trim() !== requestIdFilter) {
                                    setRequestIdFilter(requestIdInput.trim());
                                    setPage(1);
                                }
                            }}
                            placeholder="按 Request ID 精确查询"
                            className="studio-input pl-9 font-code text-[12.5px]"
                        />
                    </div>

                    {hasFilters ? (
                        <Button variant="ghost" size="sm" onClick={resetFilters}>
                            清除筛选
                        </Button>
                    ) : null}
                </form>

                {logsQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            title="无法读取审计日志"
                            error={logsQuery.error}
                            onRetry={() => void logsQuery.refetch()}
                        />
                    </div>
                ) : logsQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载审计日志…" />
                    </div>
                ) : logs.length === 0 ? (
                    <EmptyState
                        className="border-0 shadow-none"
                        title="没有符合条件的审计事件"
                        description="调整日期范围、事件类型或 Request ID 后重试。"
                        action={
                            hasFilters ? (
                                <Button variant="secondary" onClick={resetFilters}>
                                    清除筛选
                                </Button>
                            ) : null
                        }
                    />
                ) : (
                    <>
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[900px]">
                                <thead>
                                    <tr>
                                        <th>时间</th>
                                        <th>操作人</th>
                                        <th>事件</th>
                                        <th>资源</th>
                                        <th>Request ID</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {logs.map((entry) => (
                                        <tr key={entry.id}>
                                            <td className="tnum whitespace-nowrap text-[13px] text-ui-sub">
                                                {formatShortDateTime(entry.created_at)}
                                            </td>
                                            <td className="text-[13.5px]">
                                                {entry.actor ? (
                                                    <span className="font-medium text-ui-text">
                                                        {entry.actor.display_name}
                                                    </span>
                                                ) : (
                                                    <span className="text-ui-sub">系统 / 匿名</span>
                                                )}
                                            </td>
                                            <td>
                                                <span className="rounded-md bg-ui-bg px-2 py-0.5 font-code text-[12px] text-ui-text">
                                                    {entry.action}
                                                </span>
                                            </td>
                                            <td className="text-[13px]">
                                                <span className="font-code text-ui-sub">
                                                    {entry.resource_type}
                                                </span>
                                                {entry.resource_id ? (
                                                    <span className="ml-1.5 font-code text-[12px] text-ui-sub">
                                                        {shortUuid(entry.resource_id)}
                                                    </span>
                                                ) : null}
                                            </td>
                                            <td className="font-code text-[12px] text-ui-sub">
                                                {shortUuid(entry.request_id)}
                                            </td>
                                            <td className="text-right">
                                                <button
                                                    type="button"
                                                    onClick={() => setSelectedLog(entry)}
                                                    className="rounded-md px-2 py-1 text-[13px] font-medium text-accent transition-colors hover:bg-accent-tint"
                                                >
                                                    详情
                                                </button>
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </div>

                        {pagination ? (
                            <PaginationBar
                                page={pagination.page}
                                perPage={pagination.per_page}
                                total={pagination.total}
                                totalPages={pagination.total_pages}
                                onPageChange={setPage}
                            />
                        ) : null}
                    </>
                )}
            </section>

            <Drawer
                open={selectedLog !== null}
                onClose={() => setSelectedLog(null)}
                title={detail?.action ?? '审计详情'}
                subtitle={detail?.id}
                widthClassName="sm:w-[520px]"
            >
                {detail ? (
                    <div className="space-y-5">
                        <dl className="grid grid-cols-[96px_1fr] gap-y-2.5 text-[13.5px]">
                            <dt className="text-ui-sub">发生时间</dt>
                            <dd className="tnum">{formatDateTime(detail.created_at)}（UTC+8）</dd>
                            <dt className="text-ui-sub">操作人</dt>
                            <dd>
                                {detail.actor
                                    ? `${detail.actor.display_name}（${detail.actor.email}）`
                                    : '系统 / 匿名'}
                            </dd>
                            <dt className="text-ui-sub">资源类型</dt>
                            <dd className="font-code text-[12.5px]">{detail.resource_type}</dd>
                            <dt className="text-ui-sub">资源 ID</dt>
                            <dd className="font-code text-[12px] break-all">
                                {detail.resource_id ?? '—'}
                            </dd>
                            <dt className="text-ui-sub">Request ID</dt>
                            <dd className="font-code text-[12px] break-all">
                                {detail.request_id ?? '—'}
                            </dd>
                            <dt className="text-ui-sub">来源 IP</dt>
                            <dd className="tnum">{detail.ip_address ?? '—'}</dd>
                            <dt className="text-ui-sub">User-Agent</dt>
                            <dd className="text-[12px] break-all text-ui-sub">
                                {detail.user_agent ?? '—'}
                            </dd>
                        </dl>

                        <div>
                            <h3 className="text-[13.5px] font-semibold text-ui-text">
                                元数据（Metadata）
                            </h3>
                            <pre className="mt-2 overflow-x-auto rounded-xl border border-ui-line bg-ui-subtle p-3.5 font-code text-[12px] leading-6 text-ui-text">
                                {JSON.stringify(detail.metadata, null, 2)}
                            </pre>
                        </div>
                    </div>
                ) : null}
            </Drawer>
        </PageContainer>
    );
}
