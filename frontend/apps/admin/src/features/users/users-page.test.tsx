import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { ToastProvider } from '@/components/ui/toast';
import { UsersPage } from './users-page';

const { getUsers, deleteUser } = vi.hoisted(() => ({ getUsers: vi.fn(), deleteUser: vi.fn() }));

vi.mock('@/features/auth/auth-context', () => ({ useAuth: () => ({ user: { id: 'self' } }) }));
vi.mock('@/lib/api', async (original) => ({
    ...(await original<typeof import('@/lib/api')>()),
    apiClient: { GET: getUsers, DELETE: deleteUser },
}));

it('只在确认后删除指定账户，并刷新列表', async () => {
    const user = userEvent.setup();
    const buildAccount = (id: string, name: string, status: string) => ({
        id,
        display_name: name,
        email: id + '@example.test',
        status,
        roles: ['author'],
        last_login_at: null,
    });
    const self = buildAccount('self', '管理员', 'active');
    const target = buildAccount('writer', '作者', 'disabled');
    let accounts = [self, target];
    getUsers.mockImplementation(async () => ({
        data: {
            data: accounts,
            meta: { pagination: { page: 1, per_page: 20, total: accounts.length, total_pages: 1 } },
        },
        response: new Response(),
    }));
    deleteUser.mockImplementation(async () => {
        accounts = [self];
        return { response: new Response(null, { status: 204 }) };
    });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
        <QueryClientProvider client={client}>
            <ToastProvider>
                <UsersPage />
            </ToastProvider>
        </QueryClientProvider>,
    );

    const trigger = await screen.findByRole('button', { name: '删除账户「作者」' });
    expect(screen.queryByRole('button', { name: '删除账户「管理员」' })).toBeNull();
    await user.click(trigger);
    const dialog = screen.getByRole('alertdialog', { name: '删除账户「作者」？' });
    expect(dialog).toHaveTextContent('writer@example.test');
    await user.click(within(dialog).getByRole('button', { name: '取消' }));
    expect(deleteUser).not.toHaveBeenCalled();

    await user.click(trigger);
    await user.click(
        within(screen.getByRole('alertdialog')).getByRole('button', { name: '确认删除' }),
    );
    expect(deleteUser).toHaveBeenCalledExactlyOnceWith('/api/v1/admin/users/{id}', {
        params: { path: { id: 'writer' } },
    });
    await waitFor(() => expect(screen.queryByText('writer@example.test')).toBeNull());
    expect(screen.getByText('self@example.test')).toBeVisible();
});
