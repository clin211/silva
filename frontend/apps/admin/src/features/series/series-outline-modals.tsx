import { postPublicationStatus } from '@/features/posts/post-publication-status';
import type { Schemas } from '@silva/api-client';
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { AlertTriangle, CheckCircle2, ExternalLink, Search } from 'lucide-react';
import { useMemo, useState } from 'react';
import { isValidSeriesSlug } from './series-status';
import { Button } from '@/components/ui/button';
import { ErrorBanner, LoadingCard } from '@/components/ui/feedback';
import { Modal } from '@/components/ui/overlay';
import { Select } from '@/components/ui/select';
import { StatusChip } from '@/components/ui/status-chip';
import { apiClient, unwrapResponse } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { queryKeys } from '@/lib/query-keys';

export interface OutlineTargetGroupOption {
    key: string; // '__root__' or local section key / server section id
    serverId: string | null;
    label: string;
}

export function AddExistingPostsModal({
    open,
    onClose,
    existingPostIds,
    groupOptions,
    onAddPosts,
}: {
    open: boolean;
    onClose: () => void;
    existingPostIds: Set<string>;
    groupOptions: OutlineTargetGroupOption[];
    onAddPosts: (posts: Schemas['PostSummary'][], targetGroupKey: string) => void;
}) {
    const [q, setQ] = useState('');
    const [submittedQ, setSubmittedQ] = useState('');
    const [targetGroupKey, setTargetGroupKey] = useState('__root__');
    const [selectedMap, setSelectedMap] = useState<Map<string, Schemas['PostSummary']>>(
        () => new Map(),
    );

    const postsQuery = useQuery({
        queryKey: queryKeys.posts.list({
            in_series: false,
            q: submittedQ || undefined,
            page: 1,
            per_page: 50,
        }),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/posts', {
                    params: {
                        query: {
                            in_series: false,
                            q: submittedQ || undefined,
                            page: 1,
                            per_page: 50,
                        },
                    },
                }),
            ),
        enabled: open,
    });

    const candidates = useMemo(() => {
        const list = postsQuery.data?.data ?? [];
        return list.filter((post) => !existingPostIds.has(post.id));
    }, [postsQuery.data?.data, existingPostIds]);

    const effectiveTargetKey = groupOptions.some((opt) => opt.key === targetGroupKey)
        ? targetGroupKey
        : '__root__';

    const togglePost = (post: Schemas['PostSummary']) => {
        setSelectedMap((prev) => {
            const next = new Map(prev);
            if (next.has(post.id)) next.delete(post.id);
            else next.set(post.id, post);
            return next;
        });
    };

    const handleClose = () => {
        setSelectedMap(new Map());
        onClose();
    };

    const handleConfirm = () => {
        const chosen = [...selectedMap.values()];
        if (chosen.length === 0) return;
        onAddPosts(chosen, effectiveTargetKey);
        setSelectedMap(new Map());
        onClose();
    };

    return (
        <Modal
            open={open}
            onClose={handleClose}
            title="添加已有文章到目录"
            description="仅列出尚未归属任何系列的文章。添加后先缓存在本地目录中，点击「保存目录」后统一生效。"
            maxWidthClassName="max-w-[640px]"
            footer={
                <div className="flex w-full items-center justify-between gap-2">
                    <span className="tnum text-[12.5px] text-ui-sub">
                        已选 {selectedMap.size} 篇文章
                    </span>
                    <div className="flex gap-2">
                        <Button variant="secondary" onClick={handleClose}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            disabled={selectedMap.size === 0}
                            onClick={handleConfirm}
                        >
                            加入目录（{selectedMap.size}）
                        </Button>
                    </div>
                </div>
            }
        >
            <div className="space-y-3">
                <div className="grid gap-2.5 sm:grid-cols-[minmax(0,1fr)_200px]">
                    <form
                        onSubmit={(event) => {
                            event.preventDefault();
                            setSubmittedQ(q.trim());
                        }}
                        className="relative flex gap-1.5"
                    >
                        <div className="relative flex-1">
                            <Search
                                aria-hidden="true"
                                className="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[#889B98]"
                            />
                            <input
                                type="search"
                                value={q}
                                onChange={(event) => setQ(event.target.value)}
                                placeholder="按标题或 slug 搜索未归属文章…"
                                className="studio-input h-9 w-full pl-8.5 text-[13px]"
                            />
                        </div>
                        <Button variant="secondary" size="sm" type="submit">
                            搜索
                        </Button>
                    </form>

                    <Select
                        id="add-existing-target-group"
                        ariaLabel="目标位置"
                        value={effectiveTargetKey}
                        onChange={setTargetGroupKey}
                        options={groupOptions.map((opt) => ({
                            value: opt.key,
                            label: opt.label,
                        }))}
                    />
                </div>

                <div className="max-h-[340px] overflow-y-auto rounded-xl border border-ui-line">
                    {postsQuery.isError ? (
                        <div className="p-4">
                            <ErrorBanner
                                title="无法加载候选文章"
                                error={postsQuery.error}
                                onRetry={() => void postsQuery.refetch()}
                            />
                        </div>
                    ) : postsQuery.isLoading ? (
                        <div className="p-4">
                            <LoadingCard text="正在搜索未归属文章…" />
                        </div>
                    ) : candidates.length === 0 ? (
                        <p className="px-4 py-10 text-center text-[13.5px] text-ui-sub">
                            没有可添加的未归属文章。
                        </p>
                    ) : (
                        <ul className="divide-y divide-ui-line">
                            {candidates.map((post) => {
                                const checked = selectedMap.has(post.id);
                                const status = postPublicationStatus(post);
                                return (
                                    <li key={post.id}>
                                        <label
                                            className={cn(
                                                'flex cursor-pointer items-center gap-3 px-3.5 py-2.5 transition-colors',
                                                checked
                                                    ? 'bg-accent-tint/50'
                                                    : 'hover:bg-ui-subtle',
                                            )}
                                        >
                                            <input
                                                type="checkbox"
                                                checked={checked}
                                                onChange={() => togglePost(post)}
                                                className="h-4 w-4 shrink-0 accent-[#137A4D]"
                                            />
                                            <div className="min-w-0 flex-1">
                                                <div className="truncate text-[13.5px] font-medium text-ui-text">
                                                    {post.title}
                                                </div>
                                                <div className="truncate font-code text-[11.5px] text-ui-sub">
                                                    /{post.slug}
                                                </div>
                                            </div>
                                            <StatusChip tone={status.tone}>
                                                {status.label}
                                            </StatusChip>
                                        </label>
                                    </li>
                                );
                            })}
                        </ul>
                    )}
                </div>
            </div>
        </Modal>
    );
}

interface QuickCreateResult {
    post: Schemas['Post'];
    publication: Schemas['SiteVersion'] | null;
}

export function QuickCreateSeriesPostModal({
    open,
    onClose,
    series,
    outlineDirty,
    groupOptions,
    onSaveOutlineFirst,
    onRefreshSeries,
    onCompleted,
}: {
    open: boolean;
    onClose: () => void;
    series: Schemas['Series'];
    outlineDirty: boolean;
    groupOptions: OutlineTargetGroupOption[];
    /** 保存未保存的本地目录并返回最新服务端 Series 以及每个本地 groupKey 对应的真实 sectionId（根目录为 null）。 */
    onSaveOutlineFirst: () => Promise<{
        series: Schemas['Series'];
        groupKeyToSectionId: Map<string, string | null>;
    }>;
    onRefreshSeries: () => Promise<Schemas['Series']>;
    onCompleted: (result: QuickCreateResult) => void;
}) {
    const [title, setTitle] = useState('');
    const [slug, setSlug] = useState('');
    const [displayTitle, setDisplayTitle] = useState('');
    const [targetGroupKey, setTargetGroupKey] = useState('__root__');

    // 当文章创建成功但加入系列失败（或响应丢失后通过 slug 核对找到）时保留文章信息，重试只调用加入接口
    const [createdPost, setCreatedPost] = useState<Schemas['Post'] | null>(null);
    const [busyStep, setBusyStep] = useState<string | null>(null);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [ambiguousError, setAmbiguousError] = useState(false);

    const serverGroupOptions: OutlineTargetGroupOption[] = useMemo(() => {
        if (createdPost) {
            return [
                { key: '__root__', serverId: null, label: '根目录（系列顶部）' },
                ...series.sections.map((sec) => ({
                    key: sec.id,
                    serverId: sec.id,
                    label: `篇章：${sec.title}`,
                })),
            ];
        }
        return groupOptions;
    }, [createdPost, groupOptions, series.sections]);

    const effectiveTargetKey = serverGroupOptions.some((opt) => opt.key === targetGroupKey)
        ? targetGroupKey
        : '__root__';

    const resetAndClose = () => {
        setTitle('');
        setSlug('');
        setDisplayTitle('');
        setTargetGroupKey('__root__');
        setCreatedPost(null);
        setBusyStep(null);
        setErrorMsg(null);
        setAmbiguousError(false);
        onClose();
    };

    const verifyStatusAfterAmbiguousError = async () => {
        setBusyStep('正在核对服务端状态…');
        setErrorMsg(null);
        try {
            const latestSeries = await onRefreshSeries();
            const trimmedSlug = (createdPost?.slug ?? slug).trim();

            // 1. 先检查目标文章是否已经在当前系列目录中
            const allItems = [
                ...latestSeries.items,
                ...latestSeries.sections.flatMap((s) => s.items),
            ];
            const foundInSeries = allItems.find(
                (item) =>
                    (createdPost && item.post_id === createdPost.id) ||
                    (trimmedSlug && item.slug === trimmedSlug),
            );
            if (foundInSeries) {
                const postRes = await unwrapResponse(
                    apiClient.GET('/api/v1/admin/posts/{id}', {
                        params: { path: { id: foundInSeries.post_id } },
                    }),
                );
                setAmbiguousError(false);
                setBusyStep(null);
                onCompleted({ post: postRes.data, publication: null });
                resetAndClose();
                return;
            }

            // 2. 若未在系列中但文章已创建，按 slug 查找并保留文章信息，避免重复建文
            if (!createdPost && trimmedSlug) {
                const searchRes = await unwrapResponse(
                    apiClient.GET('/api/v1/admin/posts', {
                        params: { query: { q: trimmedSlug, page: 1, per_page: 20 } },
                    }),
                );
                const exact = searchRes.data.find((p) => p.slug === trimmedSlug);
                if (exact) {
                    const postRes = await unwrapResponse(
                        apiClient.GET('/api/v1/admin/posts/{id}', {
                            params: { path: { id: exact.id } },
                        }),
                    );
                    setCreatedPost(postRes.data);
                    setAmbiguousError(false);
                    setBusyStep(null);
                    setErrorMsg(
                        '核对结果：文章草稿已创建，但尚未加入系列目录。确认目标位置后可点击「重试加入系列」。',
                    );
                    return;
                }
            }

            setAmbiguousError(false);
            setBusyStep(null);
            setErrorMsg(
                createdPost
                    ? '核对完成：系列目录已刷新，文章尚未加入，请点击「重试加入系列」。'
                    : '核对完成：未找到同 Slug 文章，可重新提交创建。',
            );
        } catch (err) {
            setBusyStep(null);
            setErrorMsg(`核对失败：${formatErrorWithRequestId(err)}`);
        }
    };

    const handleSubmit = async (event: React.FormEvent) => {
        event.preventDefault();
        setErrorMsg(null);
        setAmbiguousError(false);

        if (!createdPost) {
            if (!title.trim()) {
                setErrorMsg('请输入文章标题。');
                return;
            }
            if (!isValidSeriesSlug(slug)) {
                setErrorMsg(
                    'Slug 仅允许小写字母、数字和单个连字符，不能以连字符开头或结尾（1–100 字符）。',
                );
                return;
            }
        }

        let activeSeries = series;
        let resolvedSectionId: string | null =
            serverGroupOptions.find((opt) => opt.key === effectiveTargetKey)?.serverId ?? null;

        try {
            // 1. 有未保存目录变更时，先保存以取得真实篇章 ID 和最新 edit_version
            if (!createdPost && outlineDirty) {
                setBusyStep('正在先保存未保存的目录变更…');
                const saved = await onSaveOutlineFirst();
                activeSeries = saved.series;
                resolvedSectionId = saved.groupKeyToSectionId.get(effectiveTargetKey) ?? null;
            }

            // 2. 创建草稿（若尚未创建）
            let postToAttach = createdPost;
            if (!postToAttach) {
                setBusyStep('正在创建文章草稿…');
                const createRes = await unwrapResponse(
                    apiClient.POST('/api/v1/admin/posts', {
                        body: {
                            title: title.trim(),
                            slug: slug.trim(),
                            markdown_body: '',
                            category_ids: activeSeries.category_id
                                ? [activeSeries.category_id]
                                : [],
                            tag_ids: [],
                            cover_media_id: null,
                            excerpt: null,
                            is_featured: false,
                            seo_title: null,
                            seo_description: null,
                        },
                    }),
                );
                postToAttach = createRes.data;
                setCreatedPost(postToAttach);
            } else {
                // 重试加入时先拉取最新系列状态，确保 edit_version 与目标篇章有效
                setBusyStep('正在刷新系列最新状态…');
                activeSeries = await onRefreshSeries();
                const validSection =
                    resolvedSectionId &&
                    activeSeries.sections.some((sec) => sec.id === resolvedSectionId)
                        ? resolvedSectionId
                        : null;
                resolvedSectionId = validSection;
                if (effectiveTargetKey !== '__root__' && !validSection) {
                    setTargetGroupKey('__root__');
                }
            }

            // 3. 调用单条加入接口
            setBusyStep('正在将文章加入系列目录…');
            const appendRes = await unwrapResponse(
                apiClient.POST('/api/v1/admin/series/{id}/items', {
                    params: { path: { id: activeSeries.id } },
                    body: {
                        expected_edit_version: activeSeries.edit_version,
                        post_id: postToAttach.id,
                        section_id: resolvedSectionId,
                        display_title: displayTitle.trim() || null,
                    },
                }),
            );

            setBusyStep(null);
            onCompleted({
                post: postToAttach,
                publication: appendRes.data.publication,
            });
            resetAndClose();
        } catch (err) {
            setBusyStep(null);
            const isNetworkOrServer = isApiError(err) && (err.status === 0 || err.status >= 500);
            setAmbiguousError(isNetworkOrServer);
            setErrorMsg(formatErrorWithRequestId(err));
            // 若文章已创建但加入失败，立即刷新系列以备用户重试
            if (createdPost || (isApiError(err) && err.code === 'series_edit_conflict')) {
                void onRefreshSeries().catch(() => {});
            }
        }
    };

    return (
        <Modal
            open={open}
            onClose={resetAndClose}
            title="在此系列新建文章"
            description="将创建空白草稿（自动带上系列主分类）并追加到指定位置。"
            maxWidthClassName="max-w-[520px]"
            footer={
                <div className="flex w-full items-center justify-between gap-2">
                    <div>
                        {ambiguousError ? (
                            <Button
                                variant="secondary"
                                size="sm"
                                disabled={Boolean(busyStep)}
                                onClick={() => void verifyStatusAfterAmbiguousError()}
                            >
                                核对服务端结果
                            </Button>
                        ) : null}
                    </div>
                    <div className="flex gap-2">
                        <Button
                            variant="secondary"
                            disabled={Boolean(busyStep)}
                            onClick={resetAndClose}
                        >
                            {createdPost ? '暂不加入并关闭' : '取消'}
                        </Button>
                        <Button
                            variant="primary"
                            type="submit"
                            form="quick-create-series-post-form"
                            disabled={Boolean(busyStep)}
                        >
                            {busyStep
                                ? busyStep
                                : createdPost
                                  ? '重试加入系列'
                                  : outlineDirty
                                    ? '保存目录并新建加入'
                                    : '创建并加入系列'}
                        </Button>
                    </div>
                </div>
            }
        >
            <form
                id="quick-create-series-post-form"
                onSubmit={(event) => void handleSubmit(event)}
                className="space-y-4 text-[14px]"
            >
                {outlineDirty && !createdPost ? (
                    <div className="flex items-start gap-2 rounded-lg border border-warn/30 bg-warn-tint px-3 py-2.5 text-[12.5px] text-warn">
                        <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                        <span>
                            当前目录存在未保存更改，提交时会先保存目录以获取真实篇章 ID
                            和最新版本，再创建文章并加入。
                        </span>
                    </div>
                ) : null}

                {createdPost ? (
                    <div className="rounded-lg border border-ok/30 bg-ok-tint px-3.5 py-2.5 text-[13px] text-ok">
                        <div className="flex items-center justify-between gap-2">
                            <span className="inline-flex items-center gap-1.5 font-medium">
                                <CheckCircle2 className="h-4 w-4 shrink-0" />
                                草稿已创建：{createdPost.current_revision.title}
                            </span>
                            <Link
                                to="/posts/$postId/edit"
                                params={{ postId: createdPost.id }}
                                className="inline-flex shrink-0 items-center gap-1 font-medium underline"
                            >
                                打开编辑页
                                <ExternalLink className="h-3 w-3" />
                            </Link>
                        </div>
                        <p className="mt-1 font-code text-[11.5px] opacity-90">
                            ID: {createdPost.id} · /{createdPost.slug}
                        </p>
                        <p className="mt-1 text-[12px]">
                            文章已保留，确认目标位置后点击「重试加入系列」即可，不会重复创建文章。
                        </p>
                    </div>
                ) : null}

                {errorMsg ? (
                    <div
                        role="alert"
                        className="rounded-lg border border-bad/30 bg-bad-tint px-3.5 py-2.5 text-[13px] text-bad"
                    >
                        {errorMsg}
                    </div>
                ) : null}

                {!createdPost ? (
                    <>
                        <div>
                            <label htmlFor="quick-post-title" className="block font-medium">
                                文章标题 <span className="text-bad">*</span>
                            </label>
                            <input
                                id="quick-post-title"
                                type="text"
                                maxLength={200}
                                value={title}
                                onChange={(event) => setTitle(event.target.value)}
                                placeholder="例如：线程与作用域线程"
                                className="studio-input mt-1.5"
                                required
                            />
                        </div>

                        <div>
                            <label htmlFor="quick-post-slug" className="block font-medium">
                                文章 Slug <span className="text-bad">*</span>
                            </label>
                            <input
                                id="quick-post-slug"
                                type="text"
                                maxLength={100}
                                value={slug}
                                onChange={(event) => setSlug(event.target.value)}
                                placeholder="rust-threads-and-scoped-threads"
                                className="studio-input mt-1.5 font-code text-[13px]"
                                required
                            />
                            <p className="mt-1 text-[12px] text-ui-sub">
                                公开文章地址固定为{' '}
                                <span className="font-code">/blog/{slug || 'slug'}/</span>
                                ，与系列路由独立。
                            </p>
                        </div>
                    </>
                ) : null}

                <div>
                    <label htmlFor="quick-post-target" className="block font-medium">
                        加入位置
                    </label>
                    <div className="mt-1.5">
                        <Select
                            id="quick-post-target"
                            ariaLabel="加入位置"
                            className="w-full"
                            value={effectiveTargetKey}
                            onChange={setTargetGroupKey}
                            options={serverGroupOptions.map((opt) => ({
                                value: opt.key,
                                label: opt.label,
                            }))}
                        />
                    </div>
                </div>

                <div>
                    <label htmlFor="quick-post-display-title" className="block font-medium">
                        目录短标题（可选）
                    </label>
                    <input
                        id="quick-post-display-title"
                        type="text"
                        maxLength={200}
                        value={displayTitle}
                        onChange={(event) => setDisplayTitle(event.target.value)}
                        placeholder="留空则默认使用文章标题"
                        className="studio-input mt-1.5 text-[13.5px]"
                    />
                </div>
            </form>
        </Modal>
    );
}
