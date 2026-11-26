import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { AlertTriangle, Globe, Layers, RefreshCw, Send, X } from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Select } from '@/components/ui/select';
import {
    EmptyState,
    ErrorBanner,
    LoadingCard,
    PageContainer,
    PageHeader,
    PaginationBar,
} from '@/components/ui/feedback';
import { Modal } from '@/components/ui/overlay';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { publishJobStatusMeta } from '@/features/dashboard/dashboard-page';
import { apiClient, unwrapResponse } from '@/lib/api';
import { formatErrorWithRequestId } from '@/lib/errors';
import {
    formatDateTime,
    formatGeneration,
    formatShortDateTime,
    isDelayedOver5Minutes,
} from '@/lib/format';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function PublishingJobsPage() {
    const { user } = useAuth();
    const { toast } = useToast();
    const navigate = useNavigate();
    const queryClient = useQueryClient();

    const canRequestBuild = hasPermission(user, PERMISSIONS.PUBLISHING_RETRY);
    const [page, setPage] = useState(1);
    const [statusFilter, setStatusFilter] = useState<Schemas['PublishJobStatus'] | ''>('');
    const [generationInput, setGenerationInput] = useState('');
    const [rebuildModalOpen, setRebuildModalOpen] = useState(false);

    const parsedGen = generationInput.trim()
        ? Number.parseInt(generationInput.trim(), 10)
        : undefined;
    const validGen =
        parsedGen !== undefined && Number.isFinite(parsedGen) && parsedGen > 0
            ? parsedGen
            : undefined;

    const queryParams = {
        page,
        per_page: 20,
        status: statusFilter || undefined,
        generation: validGen,
    };

    const jobsQuery = useQuery({
        queryKey: queryKeys.publishing.jobs(queryParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/publishing/jobs', {
                    params: { query: queryParams },
                }),
            ),
        refetchInterval: (query) => {
            const list = query.state.data?.data ?? [];
            return list.some((j) => j.status === 'queued' || j.status === 'building')
                ? 4000
                : false;
        },
    });

    const publicSiteQuery = useQuery({
        queryKey: queryKeys.publicSite.current,
        queryFn: async () => {
            const res = await unwrapResponse(apiClient.GET('/api/v1/public/site'));
            return res.data;
        },
        retry: false,
    });

    const rebuildMutation = useMutation({
        mutationFn: async () => {
            const res = await unwrapResponse(apiClient.POST('/api/v1/admin/publishing/jobs'));
            return res.data;
        },
        onSuccess: (target) => {
            setRebuildModalOpen(false);
            toast(`已登记全站重建任务 ${formatGeneration(target.desired_generation)}`, 'ok');
            void queryClient.invalidateQueries({ queryKey: queryKeys.publishing.all });
            void navigate({
                to: '/publishing/$jobId',
                params: { jobId: target.publish_job_id },
            });
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const jobs = jobsQuery.data?.data ?? [];
    const pagination = jobsQuery.data?.meta.pagination;
    const latestJob = jobs[0];
    const delayedJobs = jobs.filter(
        (job) =>
            (job.status === 'queued' || job.status === 'building') &&
            isDelayedOver5Minutes(job.created_at),
    );

    return (
        <PageContainer className="space-y-5">
            <PageHeader
                eyebrow="DEPLOYMENT"
                title="站点发布"
                description="文章发布、取消发布、分类标签与个人资料修改都会排队生成新的站点版本（generation），构建激活后才算上线。"
                actions={
                    canRequestBuild ? (
                        <Button variant="primary" onClick={() => setRebuildModalOpen(true)}>
                            <RefreshCw className="h-4 w-4" />
                            触发全站重建
                        </Button>
                    ) : null
                }
            />

            {/* 线上与最新目标摘要卡片 */}
            <section aria-label="线上与目标" className="grid gap-4 md:grid-cols-3">
                <div className="studio-card p-5">
                    <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-ui-sub">当前线上版本</span>
                        <span className="grid h-8 w-8 place-items-center rounded-lg bg-ok-tint text-ok">
                            <Globe className="h-4 w-4" />
                        </span>
                    </div>
                    <div className="mt-3 flex items-center gap-2.5">
                        <span className="tnum font-code text-[22px] font-semibold text-ui-text">
                            {publicSiteQuery.data
                                ? formatGeneration(publicSiteQuery.data.generation)
                                : '未上线'}
                        </span>
                        {publicSiteQuery.data ? (
                            <StatusChip tone="ok">已上线</StatusChip>
                        ) : (
                            <StatusChip tone="mute">尚未激活</StatusChip>
                        )}
                    </div>
                    <p className="tnum mt-2 text-[12px] text-ui-sub">
                        {publicSiteQuery.data
                            ? `生成于 ${formatDateTime(publicSiteQuery.data.generated_at)}`
                            : '完成首次站点构建后生效'}
                    </p>
                </div>

                <div className="studio-card p-5">
                    <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-ui-sub">最新发布任务</span>
                        <span className="grid h-8 w-8 place-items-center rounded-lg bg-accent-tint text-accent">
                            <Send className="h-4 w-4" />
                        </span>
                    </div>
                    <div className="mt-3 flex items-center gap-2.5">
                        <span className="tnum font-code text-[22px] font-semibold text-ui-text">
                            {latestJob ? formatGeneration(latestJob.generation) : '—'}
                        </span>
                        {latestJob ? (
                            <StatusChip tone={publishJobStatusMeta(latestJob.status).tone}>
                                {publishJobStatusMeta(latestJob.status).label}
                            </StatusChip>
                        ) : null}
                    </div>
                    <p className="mt-2 truncate font-code text-[12px] text-ui-sub">
                        {latestJob
                            ? `${latestJob.trigger_type} · ${formatShortDateTime(latestJob.created_at)}`
                            : '暂无任务记录'}
                    </p>
                </div>

                <div className="studio-card p-5">
                    <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-ui-sub">累计任务数</span>
                        <span className="grid h-8 w-8 place-items-center rounded-lg bg-ui-bg text-ui-sub">
                            <Layers className="h-4 w-4" />
                        </span>
                    </div>
                    <div className="tnum mt-3 text-[26px] leading-none font-semibold text-ui-text">
                        {pagination?.total ?? 0}
                    </div>
                    <p className="mt-2 text-[12px] text-ui-sub">存在进行中任务时每 4 秒自动刷新</p>
                </div>
            </section>

            {/* 超过 5 分钟未上线提示 */}
            {delayedJobs.length > 0 ? (
                <div
                    role="alert"
                    className="flex flex-wrap items-center gap-2.5 rounded-xl border border-warn/30 bg-warn-tint px-4 py-3 text-[13.5px] text-warn"
                >
                    <AlertTriangle className="h-4 w-4 shrink-0" aria-hidden="true" />
                    <span className="flex-1">
                        <strong className="font-medium">上线延迟：</strong>
                        {formatGeneration(delayedJobs[0]!.generation)} 已待上线超过 5
                        分钟。线上站点仍保持上一稳定版本，编辑与保存不受影响。
                    </span>
                    <Link
                        to="/publishing/$jobId"
                        params={{ jobId: delayedJobs[0]!.id }}
                        className="font-medium underline underline-offset-2"
                    >
                        查看任务详情
                    </Link>
                </div>
            ) : null}

            {/* 一体化任务表格卡片 */}
            <div className="studio-card overflow-hidden">
                <form
                    onSubmit={(event) => event.preventDefault()}
                    className="flex flex-wrap items-center gap-2.5 border-b border-ui-line bg-ui-subtle/70 p-3.5"
                >
                    <Select
                        id="publishing-status-filter"
                        ariaLabel="按任务状态筛选"
                        value={statusFilter}
                        onChange={(nextValue) => {
                            setStatusFilter(nextValue as Schemas['PublishJobStatus'] | '');
                            setPage(1);
                        }}
                        options={[
                            { value: '', label: '全部状态' },
                            { value: 'queued', label: '排队中' },
                            { value: 'building', label: '构建中' },
                            { value: 'active', label: '已上线' },
                            { value: 'failed', label: '构建失败' },
                            { value: 'superseded', label: '已被更新的目标替代' },
                            { value: 'retired', label: '已被新版替换' },
                        ]}
                    />

                    <label>
                        <span className="sr-only">按 generation 筛选</span>
                        <input
                            type="number"
                            min={1}
                            value={generationInput}
                            onChange={(event) => {
                                setGenerationInput(event.target.value);
                                setPage(1);
                            }}
                            placeholder="按 Generation 编号筛选"
                            className="studio-input h-9 w-52 text-[13.5px]"
                        />
                    </label>

                    {statusFilter || generationInput ? (
                        <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => {
                                setStatusFilter('');
                                setGenerationInput('');
                                setPage(1);
                            }}
                        >
                            <X className="h-3.5 w-3.5" />
                            清除筛选
                        </Button>
                    ) : null}
                </form>

                {jobsQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            title="无法读取发布任务"
                            error={jobsQuery.error}
                            onRetry={() => void jobsQuery.refetch()}
                        />
                    </div>
                ) : jobsQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载发布任务…" />
                    </div>
                ) : jobs.length === 0 ? (
                    <EmptyState
                        className="rounded-none border-0 shadow-none"
                        icon={<Send className="h-5 w-5" />}
                        title="还没有发布任务"
                        description="发布文章或保存分类标签、个人资料修改后，构建任务会出现在这里。"
                    />
                ) : (
                    <>
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[820px]">
                                <thead>
                                    <tr>
                                        <th>Generation</th>
                                        <th>触发来源</th>
                                        <th>状态</th>
                                        <th>尝试次数</th>
                                        <th>创建时间</th>
                                        <th>完成时间</th>
                                        <th>发起人</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {jobs.map((job) => {
                                        const meta = publishJobStatusMeta(job.status);
                                        const delayed =
                                            (job.status === 'queued' ||
                                                job.status === 'building') &&
                                            isDelayedOver5Minutes(job.created_at);
                                        return (
                                            <tr key={job.id}>
                                                <td className="tnum font-code font-semibold">
                                                    <Link
                                                        to="/publishing/$jobId"
                                                        params={{ jobId: job.id }}
                                                        className="text-accent hover:underline"
                                                    >
                                                        {formatGeneration(job.generation)}
                                                    </Link>
                                                </td>
                                                <td className="font-code text-[12.5px] text-ui-sub">
                                                    {job.trigger_type}
                                                </td>
                                                <td>
                                                    <StatusChip tone={delayed ? 'warn' : meta.tone}>
                                                        {delayed
                                                            ? `${meta.label} · 已延迟`
                                                            : meta.label}
                                                    </StatusChip>
                                                </td>
                                                <td className="tnum">{job.attempts}</td>
                                                <td className="tnum whitespace-nowrap text-[12.5px] text-ui-sub">
                                                    {formatShortDateTime(job.created_at)}
                                                </td>
                                                <td className="tnum whitespace-nowrap text-[12.5px] text-ui-sub">
                                                    {formatShortDateTime(job.finished_at)}
                                                </td>
                                                <td>{job.requested_by?.display_name ?? '系统'}</td>
                                                <td className="text-right">
                                                    <Link
                                                        to="/publishing/$jobId"
                                                        params={{ jobId: job.id }}
                                                        className="rounded-md px-2.5 py-1 text-[13px] font-medium text-accent hover:bg-accent-tint"
                                                    >
                                                        详情
                                                    </Link>
                                                </td>
                                            </tr>
                                        );
                                    })}
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
            </div>

            <Modal
                open={rebuildModalOpen}
                onClose={() => setRebuildModalOpen(false)}
                title="触发全站重建？"
                description="这会登记一个新的全站发布任务，导出当前所有已接受发布的文章、分类标签和个人资料并重新构建静态站点。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setRebuildModalOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={rebuildMutation.isPending}
                            onClick={() => rebuildMutation.mutate()}
                        >
                            {rebuildMutation.isPending ? '登记中…' : '确认重建'}
                        </Button>
                    </>
                }
            >
                <p className="text-[13px] text-ui-sub">
                    构建完成并激活前，线上站点保持当前版本不受影响。
                </p>
            </Modal>
        </PageContainer>
    );
}
