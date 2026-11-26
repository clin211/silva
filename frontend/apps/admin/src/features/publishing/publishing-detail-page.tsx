import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import {
    AlertTriangle,
    ArrowLeft,
    Check,
    CheckCircle2,
    Clock,
    Info,
    Loader2,
    RotateCcw,
    XCircle,
} from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { ErrorBanner, LoadingCard, PageContainer, PageHeader } from '@/components/ui/feedback';
import { Modal } from '@/components/ui/overlay';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { publishJobStatusMeta } from '@/features/dashboard/dashboard-page';
import { apiClient, unwrapResponse } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId } from '@/lib/errors';
import {
    formatDateTime,
    formatElapsed,
    formatGeneration,
    isDelayedOver5Minutes,
} from '@/lib/format';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function PublishingJobDetailPage({ jobId }: { jobId: string }) {
    const { user } = useAuth();
    const { toast } = useToast();
    const queryClient = useQueryClient();
    const canRetry = hasPermission(user, PERMISSIONS.PUBLISHING_RETRY);
    const [retryOpen, setRetryOpen] = useState(false);

    const jobQuery = useQuery({
        queryKey: queryKeys.publishing.job(jobId),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/publishing/jobs/{id}', {
                    params: { path: { id: jobId } },
                }),
            );
            return res.data;
        },
        refetchInterval: (query) => {
            const st = query.state.data?.status;
            return st === 'queued' || st === 'building' ? 3000 : false;
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

    const retryMutation = useMutation({
        mutationFn: async () => {
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/publishing/jobs/{id}/attempts', {
                    params: { path: { id: jobId } },
                }),
            );
            return res.data;
        },
        onSuccess: (updated) => {
            setRetryOpen(false);
            toast(`已重新排队：${formatGeneration(updated.generation)}`, 'ok');
            queryClient.setQueryData(queryKeys.publishing.job(jobId), updated);
            void queryClient.invalidateQueries({ queryKey: queryKeys.publishing.all });
        },
        onError: (error) => {
            setRetryOpen(false);
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    if (jobQuery.isLoading) {
        return (
            <PageContainer>
                <LoadingCard text="正在加载发布任务详情…" />
            </PageContainer>
        );
    }

    if (jobQuery.isError || !jobQuery.data) {
        return (
            <PageContainer>
                <ErrorBanner
                    title="无法读取发布任务"
                    error={jobQuery.error}
                    onRetry={() => void jobQuery.refetch()}
                />
            </PageContainer>
        );
    }

    const job = jobQuery.data;
    const statusMeta = publishJobStatusMeta(job.status);
    const delayed =
        (job.status === 'queued' || job.status === 'building') &&
        isDelayedOver5Minutes(job.created_at);
    const liveGeneration = publicSiteQuery.data?.generation ?? null;
    const liveLabel = liveGeneration !== null ? formatGeneration(liveGeneration) : '未上线';

    // 单一合并状态说明横幅（消除重复叠加的黄色横幅）
    const callout = (() => {
        if (delayed) {
            return {
                tone: 'warn' as const,
                icon: AlertTriangle,
                title: '上线延迟',
                body: `任务已处于${statusMeta.label}状态超过 5 分钟。当前线上版本仍为「${liveLabel}」，文章编辑与保存不受影响，无需重复发布。`,
            };
        }
        if (job.status === 'queued') {
            return {
                tone: 'warn' as const,
                icon: Clock,
                title: '等待 Worker 领取',
                body: `任务已在队列中，待后台 Worker 领取并执行静态站点构建。当前线上版本为「${liveLabel}」。`,
            };
        }
        if (job.status === 'building') {
            return {
                tone: 'info' as const,
                icon: Loader2,
                title: '正在构建站点',
                body: `Worker 正在导出不可变快照并执行 Astro 静态构建，激活后即切换上线。`,
            };
        }
        if (job.status === 'failed') {
            return {
                tone: 'bad' as const,
                icon: XCircle,
                title: '构建失败，线上站点未受影响',
                body: `内容修改已完整保留。${canRetry ? '请排查下方日志后点击右上角重试构建。' : '需要 Admin 检查失败原因并重试此任务。'}`,
            };
        }
        if (job.status === 'active') {
            return {
                tone: 'ok' as const,
                icon: CheckCircle2,
                title: '已激活上线',
                body: `${formatGeneration(job.generation)} 已切换为当前线上版本，公开站点与 Public API 均同步生效。`,
            };
        }
        if (job.status === 'superseded') {
            return {
                tone: 'mute' as const,
                icon: Info,
                title: '已被更新目标替代',
                body: '变更并未撤销，更新的发布目标同样包含本次修改。',
            };
        }
        return {
            tone: 'mute' as const,
            icon: Info,
            title: '历史版本已替换',
            body: '此版本曾成功上线，目前已被更新的站点版本替换。',
        };
    })();

    const CalloutIcon = callout.icon;

    return (
        <PageContainer className="space-y-5">
            <PageHeader
                eyebrow="PUBLISH JOB"
                title={formatGeneration(job.generation)}
                badge={
                    <StatusChip tone={delayed ? 'warn' : statusMeta.tone}>
                        {delayed ? `${statusMeta.label} · 已延迟` : statusMeta.label}
                    </StatusChip>
                }
                description={`触发来源：${job.trigger_type} · 发起人：${job.requested_by?.display_name ?? '系统'} · 创建于 ${formatDateTime(job.created_at)}`}
                actions={
                    <>
                        <Link
                            to="/publishing"
                            className="inline-flex h-9 items-center gap-1.5 rounded-[9px] border border-ui-line bg-white px-3.5 text-[13.5px] font-medium text-ui-text shadow-2xs hover:bg-ui-subtle"
                        >
                            <ArrowLeft className="h-4 w-4 text-ui-sub" />
                            返回列表
                        </Link>
                        {canRetry && job.status === 'failed' ? (
                            <Button variant="primary" onClick={() => setRetryOpen(true)}>
                                <RotateCcw className="h-4 w-4" />
                                重试构建
                            </Button>
                        ) : null}
                    </>
                }
            />

            {/* 单一清晰上下文提示栏 */}
            <div
                role={callout.tone === 'bad' || callout.tone === 'warn' ? 'alert' : 'status'}
                className={cn(
                    'flex items-start gap-3 rounded-xl border px-4 py-3.5 text-[13.5px]',
                    callout.tone === 'ok' && 'border-ok/25 bg-ok-tint text-ok',
                    callout.tone === 'warn' && 'border-warn/25 bg-warn-tint text-warn',
                    callout.tone === 'bad' && 'border-bad/25 bg-bad-tint text-bad',
                    callout.tone === 'info' && 'border-accent/25 bg-accent-tint text-accent-2',
                    callout.tone === 'mute' && 'border-ui-line bg-white text-ui-sub',
                )}
            >
                <CalloutIcon
                    className={cn(
                        'mt-0.5 h-4 w-4 shrink-0',
                        job.status === 'building' && !delayed && 'animate-spin',
                    )}
                    aria-hidden="true"
                />
                <div>
                    <strong className="font-semibold">{callout.title}：</strong>
                    <span>{callout.body}</span>
                </div>
            </div>

            <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
                <div className="min-w-0 space-y-5">
                    {/* 流水线时间轴 */}
                    <section aria-labelledby="job-steps-h" className="studio-card p-6">
                        <h2 id="job-steps-h" className="text-[15.5px] font-semibold text-ui-text">
                            发布流水线阶段
                        </h2>
                        <ol className="mt-5 space-y-6 text-[14px]">
                            <li className="relative pl-9">
                                <span
                                    aria-hidden="true"
                                    className="absolute top-6 -bottom-6 left-[11px] w-0.5 bg-ui-line"
                                />
                                <span className="absolute top-0 left-0 grid h-6 w-6 place-items-center rounded-full bg-ok text-white ring-4 ring-white">
                                    <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                                </span>
                                <div className="font-medium text-ui-text">任务入队</div>
                                <div className="tnum mt-0.5 text-[12.5px] text-ui-sub">
                                    {formatDateTime(job.created_at)}
                                </div>
                            </li>

                            <li className="relative pl-9">
                                <span
                                    aria-hidden="true"
                                    className="absolute top-6 -bottom-6 left-[11px] w-0.5 bg-ui-line"
                                />
                                <span
                                    className={cn(
                                        'absolute top-0 left-0 grid h-6 w-6 place-items-center rounded-full ring-4 ring-white',
                                        job.status === 'queued' &&
                                            'border-2 border-ui-line-strong bg-white text-ui-sub',
                                        job.status === 'building' &&
                                            'border-2 border-accent bg-accent-tint text-accent',
                                        job.status === 'failed' && 'bg-bad text-white',
                                        (job.status === 'active' ||
                                            job.status === 'retired' ||
                                            job.status === 'superseded') &&
                                            'bg-ok text-white',
                                    )}
                                >
                                    {job.status === 'queued' ? (
                                        <Clock className="h-3 w-3" />
                                    ) : job.status === 'building' ? (
                                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                    ) : job.status === 'failed' ? (
                                        <XCircle className="h-3.5 w-3.5" />
                                    ) : (
                                        <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                                    )}
                                </span>
                                <div className="font-medium text-ui-text">
                                    快照导出与 Astro 静态构建{' '}
                                    <span className="tnum text-[12.5px] font-normal text-ui-sub">
                                        （第 {job.attempts} 次尝试）
                                    </span>
                                </div>
                                <div className="tnum mt-0.5 text-[12.5px] text-ui-sub">
                                    {job.started_at
                                        ? `开始于 ${formatDateTime(job.started_at)}`
                                        : '等待后台 Worker 领取执行'}
                                </div>
                            </li>

                            <li className="relative pl-9">
                                <span
                                    className={cn(
                                        'absolute top-0 left-0 grid h-6 w-6 place-items-center rounded-full ring-4 ring-white',
                                        job.status === 'active' || job.status === 'retired'
                                            ? 'bg-ok text-white'
                                            : 'border-2 border-ui-line bg-white text-ui-sub',
                                    )}
                                >
                                    {job.status === 'active' || job.status === 'retired' ? (
                                        <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                                    ) : (
                                        <span className="h-1.5 w-1.5 rounded-full bg-ui-line-strong" />
                                    )}
                                </span>
                                <div className="font-medium text-ui-text">原子切换上线</div>
                                <div className="mt-0.5 text-[12.5px] text-ui-sub">
                                    {job.status === 'active'
                                        ? `已激活 · ${formatDateTime(job.finished_at)}`
                                        : job.status === 'retired'
                                          ? '曾激活，现已被更新版本替换'
                                          : job.status === 'superseded'
                                            ? '已跳过：存在更新的发布目标'
                                            : '待构建完成后激活'}
                                </div>
                            </li>
                        </ol>
                    </section>

                    {job.error_message ? (
                        <section
                            aria-labelledby="job-err-h"
                            className="studio-card overflow-hidden border-bad/30"
                        >
                            <div className="border-b border-bad/20 bg-bad-tint/60 px-5 py-3">
                                <h2 id="job-err-h" className="text-[14.5px] font-semibold text-bad">
                                    构建失败日志
                                </h2>
                            </div>
                            <pre className="overflow-x-auto bg-[#091C1A] p-4 font-code text-[12px] leading-6 whitespace-pre-wrap text-[#D7EEEA]">
                                {job.error_message}
                            </pre>
                        </section>
                    ) : null}
                </div>

                {/* 右侧信息卡片 */}
                <aside className="space-y-4">
                    <section className="studio-card p-5">
                        <h2 className="text-[14.5px] font-semibold text-ui-text">版本对照</h2>
                        <p className="mt-0.5 text-[12px] text-ui-sub">由服务端实时读取。</p>
                        <dl className="mt-4 space-y-3 border-t border-ui-line pt-4 text-[13.5px]">
                            <div className="flex items-center justify-between gap-3">
                                <dt className="text-ui-sub">本任务版本</dt>
                                <dd className="tnum font-code font-semibold text-ui-text">
                                    {formatGeneration(job.generation)}
                                </dd>
                            </div>
                            <div className="flex items-center justify-between gap-3">
                                <dt className="text-ui-sub">当前线上版本</dt>
                                <dd className="tnum font-code font-medium text-ui-text">
                                    {liveLabel}
                                </dd>
                            </div>
                            <div className="space-y-1 pt-1">
                                <dt className="text-[12px] text-ui-sub">任务 ID</dt>
                                <dd className="rounded-lg bg-ui-bg px-2.5 py-1.5 font-code text-[11.5px] break-all text-ui-sub">
                                    {job.id}
                                </dd>
                            </div>
                        </dl>
                    </section>

                    <section className="studio-card p-5">
                        <h2 className="text-[14.5px] font-semibold text-ui-text">时间记录</h2>
                        <dl className="tnum mt-3.5 grid grid-cols-[64px_1fr] gap-y-2.5 border-t border-ui-line pt-3.5 text-[13px]">
                            <dt className="text-ui-sub">创建</dt>
                            <dd className="text-right font-code text-[12.5px]">
                                {formatDateTime(job.created_at)}
                            </dd>
                            <dt className="text-ui-sub">开始</dt>
                            <dd className="text-right font-code text-[12.5px]">
                                {formatDateTime(job.started_at)}
                            </dd>
                            <dt className="text-ui-sub">结束</dt>
                            <dd className="text-right font-code text-[12.5px]">
                                {formatDateTime(job.finished_at)}
                            </dd>
                            <dt className="text-ui-sub">耗时</dt>
                            <dd className="text-right font-medium text-ui-text">
                                {formatElapsed(job.started_at ?? job.created_at, job.finished_at)}
                            </dd>
                        </dl>
                    </section>
                </aside>
            </div>

            <Modal
                open={retryOpen}
                onClose={() => setRetryOpen(false)}
                title={`重试构建 ${formatGeneration(job.generation)}？`}
                description="仅当该任务仍为最新目标时可以重试。重试会在构建成功激活前保持线上站点不变。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setRetryOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={retryMutation.isPending}
                            onClick={() => retryMutation.mutate()}
                        >
                            {retryMutation.isPending ? '提交中…' : '确认重试'}
                        </Button>
                    </>
                }
            >
                <p className="text-[13px] text-ui-sub">当前已尝试次数：{job.attempts}</p>
            </Modal>
        </PageContainer>
    );
}
