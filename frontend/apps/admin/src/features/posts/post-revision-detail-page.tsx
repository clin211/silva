import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useMemo, useState } from 'react';
import { AuthenticatedImage, SafeHtmlPreview } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import { ErrorBanner, LoadingCard, PageContainer, PageHeader } from '@/components/ui/feedback';
import { Modal } from '@/components/ui/overlay';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, unwrapResponse } from '@/lib/api';
import { formatErrorWithRequestId } from '@/lib/errors';
import { formatDateTime } from '@/lib/format';
import { sanitizeServerRenderedHtml } from '@/lib/markdown';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function PostRevisionDetailPage({
    postId,
    revisionId,
}: {
    postId: string;
    revisionId: string;
}) {
    const { user } = useAuth();
    const { toast } = useToast();
    const queryClient = useQueryClient();

    const canSubmit = hasPermission(user, PERMISSIONS.POSTS_SUBMIT_OWN);
    const canReview = hasPermission(user, PERMISSIONS.POSTS_REVIEW);

    const [submitOpen, setSubmitOpen] = useState(false);
    const [approveOpen, setApproveOpen] = useState(false);
    const [rejectOpen, setRejectOpen] = useState(false);
    const [approveNote, setApproveNote] = useState('');
    const [rejectNote, setRejectNote] = useState('');
    const [rejectError, setRejectError] = useState<string | null>(null);

    const revisionQuery = useQuery({
        queryKey: queryKeys.posts.revision(postId, revisionId),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}/revisions/{revision_id}', {
                    params: { path: { id: postId, revision_id: revisionId } },
                }),
            );
            return res.data;
        },
    });

    const reviewId = revisionQuery.data?.review?.id;

    const reviewDetailQuery = useQuery({
        queryKey: queryKeys.posts.review(postId, reviewId ?? ''),
        queryFn: async () => {
            const res = await unwrapResponse(
                apiClient.GET('/api/v1/admin/posts/{id}/reviews/{review_id}', {
                    params: { path: { id: postId, review_id: reviewId! } },
                }),
            );
            return res.data;
        },
        enabled: Boolean(reviewId),
    });

    const invalidateAll = () => {
        void queryClient.invalidateQueries({
            queryKey: queryKeys.posts.revision(postId, revisionId),
        });
        void queryClient.invalidateQueries({ queryKey: queryKeys.posts.detail(postId) });
        void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
        if (reviewId) {
            void queryClient.invalidateQueries({
                queryKey: queryKeys.posts.review(postId, reviewId),
            });
        }
    };

    const submitMutation = useMutation({
        mutationFn: async () => {
            const res = await unwrapResponse(
                apiClient.POST('/api/v1/admin/posts/{id}/reviews', {
                    params: { path: { id: postId } },
                    body: { revision_id: revisionId },
                }),
            );
            return res.data;
        },
        onSuccess: (created) => {
            setSubmitOpen(false);
            toast(`已提交 r${created.revision_no} 审核`, 'ok');
            invalidateAll();
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const decideMutation = useMutation({
        mutationFn: async (input: {
            decision: 'approved' | 'rejected';
            comment: string | null;
        }) => {
            const res = await unwrapResponse(
                apiClient.PATCH('/api/v1/admin/posts/{id}/reviews/{review_id}', {
                    params: { path: { id: postId, review_id: reviewId! } },
                    body: input,
                }),
            );
            return res.data;
        },
        onSuccess: (updated) => {
            setApproveOpen(false);
            setRejectOpen(false);
            setApproveNote('');
            setRejectNote('');
            toast(
                updated.status === 'approved'
                    ? `已批准 r${updated.revision_no}`
                    : `已拒绝 r${updated.revision_no}`,
                updated.status === 'approved' ? 'ok' : 'info',
            );
            invalidateAll();
        },
        onError: (error) => {
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const sanitizedHtml = useMemo(
        () => sanitizeServerRenderedHtml(revisionQuery.data?.rendered_html ?? ''),
        [revisionQuery.data?.rendered_html],
    );

    if (revisionQuery.isLoading) {
        return <LoadingCard text="正在加载修订详情…" />;
    }

    if (revisionQuery.isError || !revisionQuery.data) {
        return (
            <ErrorBanner
                title="无法读取修订"
                error={revisionQuery.error}
                onRetry={() => void revisionQuery.refetch()}
            />
        );
    }

    const rev = revisionQuery.data;
    const review = reviewDetailQuery.data;
    const reviewStatus = rev.review?.status ?? null;

    const statusBadge =
        reviewStatus === 'pending' ? (
            <StatusChip tone="info">审核中</StatusChip>
        ) : reviewStatus === 'approved' ? (
            <StatusChip tone="ok">已批准</StatusChip>
        ) : reviewStatus === 'rejected' ? (
            <StatusChip tone="bad">已拒绝</StatusChip>
        ) : (
            <StatusChip tone="mute">未提交</StatusChip>
        );

    return (
        <PageContainer className="space-y-5">
            <PageHeader
                eyebrow="REVISION REVIEW"
                title={`修订 r${rev.revision_no}`}
                badge={statusBadge}
                description={rev.title}
                actions={
                    <Link
                        to="/posts/$postId/revisions"
                        params={{ postId }}
                        className="inline-flex h-9 items-center rounded-[9px] border border-ui-line bg-white px-4 text-[13.5px] font-medium text-ui-text shadow-2xs hover:bg-ui-subtle"
                    >
                        返回修订历史
                    </Link>
                }
            />

            <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
                {/* 安全预览 */}
                <section
                    aria-labelledby="rev-preview-heading"
                    className="studio-card overflow-hidden"
                >
                    <div className="flex h-11 flex-wrap items-center justify-between gap-2 border-b border-ui-line px-4">
                        <h2 id="rev-preview-heading" className="text-[14px] font-semibold">
                            安全预览
                        </h2>
                        <span className="text-[12px] text-ui-sub">
                            仅后台可访问，没有公开预览地址
                        </span>
                    </div>
                    <div className="bg-paper px-5 py-8 sm:px-10">
                        <div className="mx-auto max-w-[720px]">
                            {rev.cover_media_id ? (
                                <div className="mb-6 aspect-[16/7] overflow-hidden rounded-md border border-rule bg-paper-2">
                                    <AuthenticatedImage
                                        mediaId={rev.cover_media_id}
                                        alt={`${rev.title} 封面`}
                                        className="h-full w-full object-cover"
                                    />
                                </div>
                            ) : null}

                            {rev.categories.length > 0 || rev.tags.length > 0 ? (
                                <p className="text-[13px] text-ink-soft">
                                    {rev.categories.map((c) => c.name).join(' / ')}
                                    {rev.categories.length > 0 && rev.tags.length > 0 ? ' · ' : ''}
                                    {rev.tags.map((t) => `#${t.name}`).join(' ')}
                                </p>
                            ) : null}

                            <h2 className="mt-2 font-serif text-[28px] leading-tight font-bold text-ink">
                                {rev.title}
                            </h2>
                            {rev.excerpt ? (
                                <p className="mt-3 text-[15px] leading-relaxed text-ink-soft">
                                    {rev.excerpt}
                                </p>
                            ) : null}

                            <div className="mt-6">
                                <SafeHtmlPreview html={sanitizedHtml} />
                            </div>
                        </div>
                    </div>
                </section>

                {/* 修订信息 + 审核面板 */}
                <div className="space-y-4 lg:sticky lg:top-20">
                    <section
                        aria-labelledby="rev-meta-heading"
                        className="studio-card overflow-hidden"
                    >
                        <h2
                            id="rev-meta-heading"
                            className="flex h-11 items-center border-b border-ui-line px-4 text-[14px] font-semibold"
                        >
                            修订信息
                        </h2>
                        <dl className="grid grid-cols-[88px_1fr] gap-y-2 px-4 py-3 text-[13px]">
                            <dt className="text-ui-sub">修订号</dt>
                            <dd className="tnum">r{rev.revision_no}</dd>
                            <dt className="text-ui-sub">保存时间</dt>
                            <dd className="tnum">{formatDateTime(rev.created_at)}</dd>
                            <dt className="text-ui-sub">保存者</dt>
                            <dd>{rev.created_by?.display_name ?? '—'}</dd>
                            <dt className="text-ui-sub">分类</dt>
                            <dd>
                                {rev.categories.length > 0
                                    ? rev.categories.map((c) => c.name).join('、')
                                    : '未设置'}
                            </dd>
                            <dt className="text-ui-sub">标签</dt>
                            <dd>
                                {rev.tags.length > 0
                                    ? rev.tags.map((t) => t.name).join('、')
                                    : '未设置'}
                            </dd>
                            <dt className="text-ui-sub">首页精选</dt>
                            <dd>{rev.is_featured ? '是' : '否'}</dd>
                            <dt className="text-ui-sub">SEO 标题</dt>
                            <dd className="text-ui-sub">{rev.seo_title || '（同文章标题）'}</dd>
                            <dt className="text-ui-sub">SEO 描述</dt>
                            <dd className="text-ui-sub">{rev.seo_description || '—'}</dd>
                        </dl>
                        <p className="border-t border-ui-line px-4 py-3 text-[12px] text-ui-sub">
                            正文、分类标签、封面与 SEO 都随这一修订冻结，审核结果只作用于 r
                            {rev.revision_no}。
                        </p>
                    </section>

                    <section aria-labelledby="rev-action-heading" className="studio-card p-5">
                        <h2 id="rev-action-heading" className="text-[14px] font-semibold">
                            审核
                        </h2>

                        {reviewStatus === null ? (
                            <div className="mt-3">
                                <p className="text-[13px] text-ui-sub">这个修订还没有提交审核。</p>
                                {canSubmit ? (
                                    <Button
                                        variant="primary"
                                        className="mt-3 w-full"
                                        onClick={() => setSubmitOpen(true)}
                                    >
                                        提交 r{rev.revision_no} 审核
                                    </Button>
                                ) : null}
                            </div>
                        ) : null}

                        {reviewStatus === 'pending' ? (
                            <div className="mt-3">
                                {canReview ? (
                                    <>
                                        <p className="text-[13px] text-ui-sub">
                                            批准后它可以被选为发布版本；拒绝不会撤下当前已上线的版本。
                                        </p>
                                        <div className="mt-3 grid grid-cols-2 gap-2">
                                            <Button
                                                variant="danger-outline"
                                                onClick={() => {
                                                    setRejectError(null);
                                                    setRejectOpen(true);
                                                }}
                                            >
                                                拒绝
                                            </Button>
                                            <Button
                                                variant="ok"
                                                onClick={() => setApproveOpen(true)}
                                            >
                                                批准 r{rev.revision_no}
                                            </Button>
                                        </div>
                                    </>
                                ) : (
                                    <p className="text-[13px] text-ui-sub">
                                        已提交审核，等待 Editor
                                        处理。你可以继续编辑，新的保存会生成新修订，不影响 r
                                        {rev.revision_no} 的审核。
                                    </p>
                                )}
                            </div>
                        ) : null}

                        {reviewStatus === 'approved' ? (
                            <div className="mt-3 text-[13px]">
                                <p className="text-ok">
                                    {review?.reviewer?.display_name ?? '审核人'} 于{' '}
                                    {formatDateTime(review?.reviewed_at)} 批准。
                                </p>
                                {review?.comment ? (
                                    <blockquote className="mt-2 border-l-2 border-ok/40 pl-3 text-ui-sub">
                                        {review.comment}
                                    </blockquote>
                                ) : null}
                                {canReview ? (
                                    <Link
                                        to="/posts/$postId/edit"
                                        params={{ postId }}
                                        className="mt-3 inline-flex h-9 w-full items-center justify-center rounded-md bg-accent text-[14px] font-medium text-white hover:bg-accent-2"
                                    >
                                        去编辑页发布 r{rev.revision_no}
                                    </Link>
                                ) : null}
                            </div>
                        ) : null}

                        {reviewStatus === 'rejected' ? (
                            <div className="mt-3 text-[13px]">
                                <p className="text-bad">
                                    {review?.reviewer?.display_name ?? '审核人'} 于{' '}
                                    {formatDateTime(review?.reviewed_at)} 拒绝。
                                </p>
                                {review?.comment ? (
                                    <blockquote className="mt-2 border-l-2 border-bad/40 pl-3 text-ui-sub">
                                        {review.comment}
                                    </blockquote>
                                ) : null}
                                <Link
                                    to="/posts/$postId/edit"
                                    params={{ postId }}
                                    className="mt-3 inline-flex h-9 w-full items-center justify-center rounded-md border border-ui-line text-[14px] text-ui-text hover:bg-ui-bg"
                                >
                                    回到编辑修改
                                </Link>
                            </div>
                        ) : null}
                    </section>
                </div>
            </div>

            {/* 提交审核 */}
            <Modal
                open={submitOpen}
                onClose={() => setSubmitOpen(false)}
                title={`提交 r${rev.revision_no} 审核？`}
                description={`审核只作用于 r${rev.revision_no}，之后的修改会生成新修订，不继承本次审核结果。`}
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setSubmitOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={submitMutation.isPending}
                            onClick={() => submitMutation.mutate()}
                        >
                            {submitMutation.isPending ? '提交中…' : `提交 r${rev.revision_no} 审核`}
                        </Button>
                    </>
                }
            >
                <div className="text-[13px] text-ui-sub">标题：{rev.title}</div>
            </Modal>

            {/* 批准 */}
            <Modal
                open={approveOpen}
                onClose={() => setApproveOpen(false)}
                title={`批准修订 r${rev.revision_no}？`}
                description={`批准不会自动发布。之后可在编辑页的「发布」中选择 r${rev.revision_no}，进入站点构建后才会上线。`}
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setApproveOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="ok"
                            disabled={decideMutation.isPending}
                            onClick={() =>
                                decideMutation.mutate({
                                    decision: 'approved',
                                    comment: approveNote.trim() || null,
                                })
                            }
                        >
                            {decideMutation.isPending ? '处理中…' : `批准 r${rev.revision_no}`}
                        </Button>
                    </>
                }
            >
                <label htmlFor="approve-note" className="block text-[13px] text-ui-sub">
                    审核意见（可选）
                </label>
                <textarea
                    id="approve-note"
                    rows={3}
                    maxLength={2000}
                    value={approveNote}
                    onChange={(event) => setApproveNote(event.target.value)}
                    className="mt-1.5 w-full rounded-md border border-ui-line px-3 py-2 text-[14px] focus:border-accent focus:outline-none"
                />
            </Modal>

            {/* 拒绝 */}
            <Modal
                open={rejectOpen}
                onClose={() => setRejectOpen(false)}
                title={`拒绝修订 r${rev.revision_no}？`}
                description="作者会看到你的拒绝原因。当前已上线的版本不受影响。"
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setRejectOpen(false)}>
                            取消
                        </Button>
                        <Button
                            variant="danger"
                            disabled={decideMutation.isPending}
                            onClick={() => {
                                if (!rejectNote.trim()) {
                                    setRejectError('请写明需要修改的原因。');
                                    return;
                                }
                                setRejectError(null);
                                decideMutation.mutate({
                                    decision: 'rejected',
                                    comment: rejectNote.trim(),
                                });
                            }}
                        >
                            {decideMutation.isPending ? '处理中…' : `拒绝 r${rev.revision_no}`}
                        </Button>
                    </>
                }
            >
                <label htmlFor="reject-note" className="block text-[13px] font-medium text-ui-text">
                    拒绝原因（必填）
                </label>
                <textarea
                    id="reject-note"
                    rows={3}
                    maxLength={2000}
                    value={rejectNote}
                    onChange={(event) => {
                        setRejectNote(event.target.value);
                        if (rejectError) setRejectError(null);
                    }}
                    className="mt-1.5 w-full rounded-md border border-ui-line px-3 py-2 text-[14px] focus:border-accent focus:outline-none"
                />
                {rejectError ? <p className="mt-1 text-[13px] text-bad">{rejectError}</p> : null}
            </Modal>
        </PageContainer>
    );
}
