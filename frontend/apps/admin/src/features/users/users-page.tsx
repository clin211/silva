import { zodResolver } from '@hookform/resolvers/zod';
import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Search, UserPlus } from 'lucide-react';
import { useState } from 'react';
import { useForm, useWatch } from 'react-hook-form';
import { z } from 'zod';
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
import { Drawer, Modal } from '@/components/ui/overlay';
import { StatusChip } from '@/components/ui/status-chip';
import { useToast } from '@/components/ui/toast';
import { useAuth } from '@/features/auth/auth-context';
import { apiClient, unwrapResponse } from '@/lib/api';
import { applyApiFieldErrors, formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { formatShortDateTime, utf8ByteLength } from '@/lib/format';
import { ROLE_DESCRIPTIONS, type RoleKey } from '@/lib/permissions';
import { queryKeys } from '@/lib/query-keys';

const ALL_ROLES: RoleKey[] = ['author', 'editor', 'admin'];

const userFormSchema = z.object({
    display_name: z
        .string()
        .trim()
        .min(1, '请输入显示名称')
        .refine((v) => utf8ByteLength(v) <= 100, '显示名称不能超过 100 字节'),
    email: z.string().trim().email('请输入有效邮箱'),
    password: z.string(),
    roles: z.array(z.enum(['author', 'editor', 'admin'])).min(1, '至少选择一个内置角色'),
});

type UserFormValues = z.infer<typeof userFormSchema>;

export function UsersPage() {
    const { user: currentUser } = useAuth();
    const { toast } = useToast();
    const queryClient = useQueryClient();

    const [page, setPage] = useState(1);
    const [qInput, setQInput] = useState('');
    const [q, setQ] = useState('');
    const [roleFilter, setRoleFilter] = useState<RoleKey | ''>('');
    const [statusFilter, setStatusFilter] = useState<Schemas['AccountStatus'] | ''>('');

    const [drawerMode, setDrawerMode] = useState<
        { mode: 'create' } | { mode: 'edit'; target: Schemas['User'] } | null
    >(null);
    const [statusModalTarget, setStatusModalTarget] = useState<Schemas['User'] | null>(null);
    const [deleteTarget, setDeleteTarget] = useState<Schemas['User'] | null>(null);
    const [formTopError, setFormTopError] = useState<string | null>(null);

    const queryParams = {
        page,
        per_page: 20,
        q: q || undefined,
        role: roleFilter || undefined,
        status: statusFilter || undefined,
    };

    const usersQuery = useQuery({
        queryKey: queryKeys.users.list(queryParams),
        queryFn: async () =>
            unwrapResponse(
                apiClient.GET('/api/v1/admin/users', {
                    params: { query: queryParams },
                }),
            ),
    });

    const {
        register,
        control,
        handleSubmit,
        reset,
        setValue,
        setError,
        formState: { errors },
    } = useForm<UserFormValues>({
        resolver: zodResolver(userFormSchema),
        defaultValues: { display_name: '', email: '', password: '', roles: ['author'] },
    });

    const openUserDrawer = (
        mode: { mode: 'create' } | { mode: 'edit'; target: Schemas['User'] },
    ) => {
        setFormTopError(null);
        setDrawerMode(mode);
        if (mode.mode === 'create') {
            reset({ display_name: '', email: '', password: '', roles: ['author'] });
        } else {
            reset({
                display_name: mode.target.display_name,
                email: mode.target.email,
                password: '',
                roles: mode.target.roles,
            });
        }
    };

    const saveMutation = useMutation({
        mutationFn: async (values: UserFormValues) => {
            if (!drawerMode) return;
            if (drawerMode.mode === 'create') {
                await unwrapResponse(
                    apiClient.POST('/api/v1/admin/users', {
                        body: {
                            email: values.email.trim(),
                            display_name: values.display_name.trim(),
                            password: values.password,
                            roles: values.roles,
                        },
                    }),
                );
            } else {
                const body: Schemas['UpdateUserInput'] = {
                    display_name: values.display_name.trim(),
                    roles: values.roles,
                };
                if (values.password.length > 0) {
                    body.password = values.password;
                }
                await unwrapResponse(
                    apiClient.PATCH('/api/v1/admin/users/{id}', {
                        params: { path: { id: drawerMode.target.id } },
                        body,
                    }),
                );
            }
        },
        onSuccess: () => {
            toast(drawerMode?.mode === 'create' ? '已创建账户' : '已保存账户修改', 'ok');
            setDrawerMode(null);
            void queryClient.invalidateQueries({ queryKey: queryKeys.users.all });
            void queryClient.invalidateQueries({ queryKey: queryKeys.roles.list });
        },
        onError: (error) => {
            const unmatched = applyApiFieldErrors(error, setError);
            if (unmatched.length > 0 || !isApiError(error) || error.code !== 'validation_error') {
                setFormTopError(formatErrorWithRequestId(error));
            }
        },
    });

    const toggleStatusMutation = useMutation({
        mutationFn: async (target: Schemas['User']) => {
            const nextStatus: Schemas['AccountStatus'] =
                target.status === 'active' ? 'disabled' : 'active';
            const res = await unwrapResponse(
                apiClient.PATCH('/api/v1/admin/users/{id}', {
                    params: { path: { id: target.id } },
                    body: { status: nextStatus },
                }),
            );
            return res.data;
        },
        onSuccess: (updated) => {
            setStatusModalTarget(null);
            toast(
                updated.status === 'disabled'
                    ? `已停用 ${updated.display_name}，其所有会话已失效`
                    : `已启用 ${updated.display_name}`,
                'ok',
            );
            void queryClient.invalidateQueries({ queryKey: queryKeys.users.all });
        },
        onError: (error) => {
            setStatusModalTarget(null);
            toast(formatErrorWithRequestId(error), 'bad');
        },
    });

    const selectedRoles = useWatch({ control, name: 'roles' });
    const users = usersQuery.data?.data ?? [];
    const pagination = usersQuery.data?.meta.pagination;

    const deleteMutation = useMutation({
        mutationFn: (target: Schemas['User']) =>
            unwrapResponse(
                apiClient.DELETE('/api/v1/admin/users/{id}', {
                    params: { path: { id: target.id } },
                }),
            ),
        onSuccess: (_, target) => {
            setDeleteTarget(null);
            toast(`已删除账户「${target.display_name}」`, 'ok');
            if (users.length === 1 && page > 1) setPage(page - 1);
            queryClient.removeQueries({ queryKey: queryKeys.users.detail(target.id) });
        },
        onError: (error) => {
            setDeleteTarget(null);
            toast(formatErrorWithRequestId(error), 'bad');
        },
        onSettled: () => {
            for (const queryKey of [
                queryKeys.users.all,
                queryKeys.roles.list,
                queryKeys.auditLogs.all,
            ]) {
                void queryClient.invalidateQueries({ queryKey });
            }
        },
    });

    const handleFormSubmit = (values: UserFormValues) => {
        setFormTopError(null);
        if (drawerMode?.mode === 'create') {
            if (utf8ByteLength(values.password) < 12) {
                setError('password', { message: '初始密码至少需要 12 个字节' });
                return;
            }
        } else if (values.password.length > 0 && utf8ByteLength(values.password) < 12) {
            setError('password', { message: '重置密码至少需要 12 个字节' });
            return;
        }
        saveMutation.mutate(values);
    };

    const toggleRole = (role: RoleKey) => {
        const next = selectedRoles.includes(role)
            ? selectedRoles.filter((r) => r !== role)
            : [...selectedRoles, role];
        setValue('roles', next, { shouldValidate: true });
    };

    return (
        <PageContainer className="space-y-6">
            <PageHeader
                eyebrow="MEMBERS"
                title="账户"
                description="创建后台账户、分配内置角色、重置密码、启停或删除账户。站点不提供公开注册。"
                actions={
                    <Button variant="primary" onClick={() => openUserDrawer({ mode: 'create' })}>
                        <UserPlus className="h-4 w-4" />
                        新建账户
                    </Button>
                }
            />

            <section className="studio-card overflow-hidden">
                <form
                    onSubmit={(event) => {
                        event.preventDefault();
                        setQ(qInput.trim());
                        setPage(1);
                    }}
                    className="flex flex-wrap items-center gap-2.5 border-b border-ui-line bg-ui-subtle/60 px-4 py-3 sm:px-5"
                >
                    <div className="relative w-full sm:w-64">
                        <label className="sr-only" htmlFor="user-search-q">
                            搜索姓名或邮箱
                        </label>
                        <Search className="pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-ui-sub" />
                        <input
                            id="user-search-q"
                            type="search"
                            value={qInput}
                            onChange={(event) => setQInput(event.target.value)}
                            onBlur={() => {
                                if (qInput.trim() !== q) {
                                    setQ(qInput.trim());
                                    setPage(1);
                                }
                            }}
                            placeholder="搜索姓名或邮箱"
                            className="studio-input pl-9"
                        />
                    </div>

                    <Select
                        id="user-filter-role"
                        ariaLabel="按角色筛选"
                        value={roleFilter}
                        onChange={(nextValue) => {
                            setRoleFilter(nextValue as RoleKey | '');
                            setPage(1);
                        }}
                        options={[
                            { value: '', label: '全部角色' },
                            { value: 'admin', label: 'Admin' },
                            { value: 'editor', label: 'Editor' },
                            { value: 'author', label: 'Author' },
                        ]}
                    />

                    <Select
                        id="user-filter-status"
                        ariaLabel="按状态筛选"
                        value={statusFilter}
                        onChange={(nextValue) => {
                            setStatusFilter(nextValue as Schemas['AccountStatus'] | '');
                            setPage(1);
                        }}
                        options={[
                            { value: '', label: '全部状态' },
                            { value: 'active', label: '启用' },
                            { value: 'disabled', label: '已停用' },
                        ]}
                    />
                </form>

                {usersQuery.isError ? (
                    <div className="p-5">
                        <ErrorBanner
                            title="无法读取账户列表"
                            error={usersQuery.error}
                            onRetry={() => void usersQuery.refetch()}
                        />
                    </div>
                ) : usersQuery.isLoading ? (
                    <div className="p-5">
                        <LoadingCard text="正在加载账户列表…" />
                    </div>
                ) : users.length === 0 ? (
                    <EmptyState
                        title="没有符合条件的账户"
                        description="调整搜索关键词或筛选条件，或创建新的后台成员。"
                        className="border-0 shadow-none"
                    />
                ) : (
                    <>
                        <div className="overflow-x-auto">
                            <table className="tbl min-w-[820px]">
                                <thead>
                                    <tr>
                                        <th>成员</th>
                                        <th>邮箱</th>
                                        <th>角色</th>
                                        <th>状态</th>
                                        <th>最近登录</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {users.map((item) => {
                                        const isSelf = item.id === currentUser?.id;
                                        const isDisabled = item.status === 'disabled';
                                        const initial =
                                            item.display_name.trim().slice(0, 1).toUpperCase() ||
                                            'U';
                                        return (
                                            <tr key={item.id}>
                                                <td aria-label={item.display_name}>
                                                    <div className="flex items-center gap-2.5">
                                                        <span
                                                            className={
                                                                isDisabled
                                                                    ? 'grid h-7 w-7 shrink-0 place-items-center rounded-full bg-ui-bg text-[12px] font-semibold text-ui-sub'
                                                                    : 'grid h-7 w-7 shrink-0 place-items-center rounded-full bg-accent-tint text-[12px] font-semibold text-accent'
                                                            }
                                                            aria-hidden="true"
                                                        >
                                                            {initial}
                                                        </span>
                                                        <span
                                                            className={
                                                                isDisabled
                                                                    ? 'font-medium text-ui-sub'
                                                                    : 'font-medium text-ui-text'
                                                            }
                                                        >
                                                            {item.display_name}
                                                            {isSelf ? (
                                                                <span className="ml-1.5 rounded-full bg-accent-tint px-1.5 py-0.5 text-[11px] font-medium text-accent">
                                                                    当前账户
                                                                </span>
                                                            ) : null}
                                                        </span>
                                                    </div>
                                                </td>
                                                <td
                                                    className={
                                                        isDisabled
                                                            ? 'font-code text-[12.5px] text-ui-sub'
                                                            : 'font-code text-[12.5px] text-ui-text'
                                                    }
                                                >
                                                    {item.email}
                                                </td>
                                                <td>
                                                    <div className="flex flex-wrap gap-1">
                                                        {item.roles.map((role) => (
                                                            <span
                                                                key={role}
                                                                className="rounded-md bg-ui-bg px-2 py-0.5 text-[12px] font-medium text-ui-sub"
                                                            >
                                                                {ROLE_DESCRIPTIONS[role].title}
                                                            </span>
                                                        ))}
                                                    </div>
                                                </td>
                                                <td>
                                                    {isDisabled ? (
                                                        <StatusChip tone="mute">已停用</StatusChip>
                                                    ) : (
                                                        <StatusChip tone="ok">启用</StatusChip>
                                                    )}
                                                </td>
                                                <td className="tnum text-[13px] text-ui-sub">
                                                    {formatShortDateTime(item.last_login_at)}
                                                </td>
                                                <td className="whitespace-nowrap text-right">
                                                    <button
                                                        type="button"
                                                        onClick={() =>
                                                            openUserDrawer({
                                                                mode: 'edit',
                                                                target: item,
                                                            })
                                                        }
                                                        className="rounded-md px-2 py-1 text-[13px] font-medium text-accent transition-colors hover:bg-accent-tint"
                                                    >
                                                        编辑
                                                    </button>
                                                    {isSelf ? (
                                                        <span
                                                            className="px-2 text-[12px] text-ui-sub"
                                                            title="不能停用当前登录账户"
                                                        >
                                                            不可停用
                                                        </span>
                                                    ) : (
                                                        <button
                                                            type="button"
                                                            onClick={() =>
                                                                setStatusModalTarget(item)
                                                            }
                                                            className={
                                                                isDisabled
                                                                    ? 'rounded-md px-2 py-1 text-[13px] font-medium text-accent transition-colors hover:bg-accent-tint'
                                                                    : 'rounded-md px-2 py-1 text-[13px] font-medium text-bad transition-colors hover:bg-bad-tint'
                                                            }
                                                        >
                                                            {isDisabled ? '启用' : '停用'}
                                                        </button>
                                                    )}
                                                    {!isSelf && (
                                                        <button
                                                            type="button"
                                                            aria-label={`删除账户「${item.display_name}」`}
                                                            onClick={() => setDeleteTarget(item)}
                                                            className="rounded-md px-2 py-1 text-[13px] font-medium text-bad hover:bg-bad-tint"
                                                        >
                                                            删除
                                                        </button>
                                                    )}
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
                                unit="个账户"
                                onPageChange={setPage}
                            />
                        ) : null}

                        <p className="border-t border-ui-line bg-ui-subtle/70 px-5 py-3 text-[12px] text-ui-sub">
                            停用后该账户的全部会话立即失效，已写的文章与修订保留。
                        </p>
                    </>
                )}
            </section>

            {/* 创建 / 编辑抽屉 */}
            <Drawer
                open={drawerMode !== null}
                onClose={() => setDrawerMode(null)}
                title={drawerMode?.mode === 'create' ? '新建账户' : '编辑账户'}
                footer={
                    <div className="flex justify-end gap-2">
                        <Button variant="secondary" onClick={() => setDrawerMode(null)}>
                            取消
                        </Button>
                        <Button
                            variant="primary"
                            type="submit"
                            form="user-drawer-form"
                            disabled={saveMutation.isPending}
                        >
                            {saveMutation.isPending
                                ? '保存中…'
                                : drawerMode?.mode === 'create'
                                  ? '创建账户'
                                  : '保存修改'}
                        </Button>
                    </div>
                }
            >
                <form
                    id="user-drawer-form"
                    className="space-y-4"
                    onSubmit={handleSubmit(handleFormSubmit)}
                    noValidate
                >
                    {formTopError ? (
                        <div
                            role="alert"
                            className="rounded-md border border-[#E8C3BF] bg-bad-tint px-3 py-2 text-[13px] text-bad"
                        >
                            {formTopError}
                        </div>
                    ) : null}

                    <div>
                        <label
                            htmlFor="u-display-name"
                            className="block text-[13px] font-medium text-ui-text"
                        >
                            显示名称
                        </label>
                        <input
                            id="u-display-name"
                            type="text"
                            className="studio-input mt-1.5"
                            {...register('display_name')}
                        />
                        {errors.display_name ? (
                            <p className="mt-1 text-[12px] text-bad">
                                {errors.display_name.message}
                            </p>
                        ) : null}
                    </div>

                    <div>
                        <label
                            htmlFor="u-email"
                            className="block text-[13px] font-medium text-ui-text"
                        >
                            邮箱
                        </label>
                        <input
                            id="u-email"
                            type="email"
                            readOnly={drawerMode?.mode === 'edit'}
                            className="studio-input mt-1.5"
                            {...register('email')}
                        />
                        <p className="mt-1 text-[12px] text-ui-sub">
                            {drawerMode?.mode === 'edit'
                                ? '登录邮箱创建后不可更改。'
                                : '用于登录，不能与已有账户重复。'}
                        </p>
                        {errors.email ? (
                            <p className="mt-1 text-[12px] text-bad">{errors.email.message}</p>
                        ) : null}
                    </div>

                    <div>
                        <label
                            htmlFor="u-password"
                            className="block text-[13px] font-medium text-ui-text"
                        >
                            {drawerMode?.mode === 'create' ? '初始密码' : '重置密码（可选）'}
                        </label>
                        <input
                            id="u-password"
                            type="password"
                            autoComplete="new-password"
                            placeholder={drawerMode?.mode === 'edit' ? '留空则保持原密码不变' : ''}
                            className="studio-input mt-1.5"
                            {...register('password')}
                        />
                        <p className="mt-1 text-[12px] text-ui-sub">
                            至少 12 个字节。
                            {drawerMode?.mode === 'edit'
                                ? '重置密码后该账户的全部现有会话会立即失效。'
                                : '请通过安全渠道告知对方。'}
                        </p>
                        {errors.password ? (
                            <p className="mt-1 text-[12px] text-bad">{errors.password.message}</p>
                        ) : null}
                    </div>

                    <fieldset>
                        <legend className="text-[13px] font-medium text-ui-text">内置角色</legend>
                        <div className="mt-2 space-y-2">
                            {ALL_ROLES.map((roleKey) => {
                                const checked = selectedRoles.includes(roleKey);
                                const desc = ROLE_DESCRIPTIONS[roleKey];
                                const inputId = `user-role-${roleKey}`;
                                return (
                                    <label
                                        key={roleKey}
                                        htmlFor={inputId}
                                        aria-label={desc.title}
                                        className="flex cursor-pointer gap-3 rounded-xl border border-ui-line p-3 transition-colors has-[:checked]:border-accent has-[:checked]:bg-accent-tint/50"
                                    >
                                        <input
                                            id={inputId}
                                            type="checkbox"
                                            checked={checked}
                                            onChange={() => toggleRole(roleKey)}
                                            className="mt-1 accent-[#087F89]"
                                        />
                                        <span>
                                            <span className="block text-[14px] font-medium">
                                                {desc.title}
                                            </span>
                                            <span className="block text-[12px] text-ui-sub">
                                                {desc.summary}
                                            </span>
                                        </span>
                                    </label>
                                );
                            })}
                        </div>
                        {errors.roles ? (
                            <p className="mt-1 text-[12px] text-bad">{errors.roles.message}</p>
                        ) : null}
                        <p className="mt-2 text-[12px] text-ui-sub">
                            权限定义不可编辑，详见{' '}
                            <Link to="/roles" className="text-accent underline">
                                角色权限矩阵
                            </Link>
                            。
                        </p>
                    </fieldset>
                </form>
            </Drawer>

            <Modal
                open={deleteTarget !== null}
                onClose={() => {
                    if (!deleteMutation.isPending) setDeleteTarget(null);
                }}
                role="alertdialog"
                title={`删除账户「${deleteTarget?.display_name ?? ''}」？`}
                footer={
                    <>
                        <Button
                            variant="secondary"
                            disabled={deleteMutation.isPending}
                            onClick={() => setDeleteTarget(null)}
                        >
                            取消
                        </Button>
                        <Button
                            variant="danger"
                            disabled={deleteMutation.isPending}
                            onClick={() => {
                                if (deleteTarget) deleteMutation.mutate(deleteTarget);
                            }}
                        >
                            {deleteMutation.isPending ? '删除中…' : '确认删除'}
                        </Button>
                    </>
                }
            >
                <p className="text-[14px] leading-6 text-ui-sub">
                    删除后，{deleteTarget?.email}{' '}
                    可用于新建账户。原账户无法恢复，所有会话立即失效。文章、修订、媒体和审计记录保留原归属。
                </p>
            </Modal>

            {/* 停用 / 启用确认弹窗 */}
            <Modal
                open={statusModalTarget !== null}
                onClose={() => setStatusModalTarget(null)}
                role="alertdialog"
                title={
                    statusModalTarget
                        ? statusModalTarget.status === 'active'
                            ? `停用 ${statusModalTarget.display_name}？`
                            : `启用 ${statusModalTarget.display_name}？`
                        : ''
                }
                footer={
                    <>
                        <Button variant="secondary" onClick={() => setStatusModalTarget(null)}>
                            取消
                        </Button>
                        <Button
                            variant={statusModalTarget?.status === 'active' ? 'danger' : 'primary'}
                            disabled={toggleStatusMutation.isPending}
                            onClick={() => {
                                if (statusModalTarget) {
                                    toggleStatusMutation.mutate(statusModalTarget);
                                }
                            }}
                        >
                            {toggleStatusMutation.isPending
                                ? '处理中…'
                                : statusModalTarget?.status === 'active'
                                  ? '确认停用'
                                  : '确认启用'}
                        </Button>
                    </>
                }
            >
                <p className="text-[14px] leading-6 text-ui-sub">
                    {statusModalTarget?.status === 'active'
                        ? '停用后对方无法登录，当前会话立即失效。已写的文章、修订和媒体都会保留，之后可随时重新启用。'
                        : '启用后对方可以使用原邮箱和密码重新登录后台。'}
                </p>
            </Modal>
        </PageContainer>
    );
}
