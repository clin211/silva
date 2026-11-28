import type { Schemas } from '@silva/api-client';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Modal } from '@/components/ui/overlay';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, unwrapResponse } from '@/lib/api';
import { formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { hasPermission, PERMISSIONS } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

export function usePostDeletion(onDeleted?: () => void) {
    const { user } = useAuth();
    const queryClient = useQueryClient();
    const { toast } = useToast();
    const [deleteTarget, setDeleteTarget] = useState<Schemas['PostSummary'] | null>(null);
    const deletePost = useMutation({
        mutationFn: (post: Schemas['PostSummary']) =>
            unwrapResponse(
                apiClient.DELETE('/api/v1/admin/posts/{id}', {
                    params: {
                        path: { id: post.id },
                        query: {
                            expected_revision_id: post.current_revision_id,
                            expected_publication_version: post.publication_version,
                        },
                    },
                }),
            ),
        onSuccess: (_, post) => {
            setDeleteTarget(null);
            toast(`已删除「${post.title}」`, 'ok');
            onDeleted?.();
            queryClient.removeQueries({ queryKey: queryKeys.posts.detail(post.id) });
            for (const queryKey of [
                queryKeys.posts.all,
                queryKeys.taxonomy.categories,
                queryKeys.taxonomy.tags,
                queryKeys.media.all,
                queryKeys.auditLogs.all,
            ]) {
                void queryClient.invalidateQueries({ queryKey });
            }
        },
        onError: (error) => {
            setDeleteTarget(null);
            toast(
                isApiError(error) && error.code === 'revision_conflict'
                    ? '文章已被修改，请核对最新内容后再删除。'
                    : formatErrorWithRequestId(error),
                'bad',
            );
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
        },
    });

    const requestDelete = (post: Schemas['PostSummary']) => {
        if (post.published_revision_id || post.series) {
            const reason = post.published_revision_id
                ? '请先到编辑页取消发布，再删除文章。'
                : '请先从系列目录移出这篇文章，再删除。';
            toast(reason, 'warn');
            void queryClient.invalidateQueries({ queryKey: queryKeys.posts.all });
            return;
        }
        setDeleteTarget(post);
    };

    return {
        renderButton: (post: Schemas['PostSummary']) =>
            hasPermission(user, PERMISSIONS.POSTS_MANAGE) ? (
                <button
                    type="button"
                    aria-label={`删除「${post.title}」`}
                    className="rounded-md px-2 py-1 text-[13px] font-medium text-bad hover:bg-bad-tint"
                    onClick={() => requestDelete(post)}
                >
                    删除
                </button>
            ) : null,
        dialog: (
            <Modal
                open={deleteTarget !== null}
                onClose={() => {
                    if (!deletePost.isPending) setDeleteTarget(null);
                }}
                role="alertdialog"
                title={`删除文章「${deleteTarget?.title ?? ''}」？`}
                footer={
                    <>
                        <Button
                            variant="secondary"
                            disabled={deletePost.isPending}
                            onClick={() => setDeleteTarget(null)}
                        >
                            取消
                        </Button>
                        <Button
                            variant="danger"
                            disabled={deletePost.isPending}
                            onClick={() => {
                                if (deleteTarget) deletePost.mutate(deleteTarget);
                            }}
                        >
                            {deletePost.isPending ? '删除中…' : '确认删除'}
                        </Button>
                    </>
                }
            >
                <p className="text-sm text-ui-sub">文章内容将永久删除，无法恢复。</p>
            </Modal>
        ),
    };
}
