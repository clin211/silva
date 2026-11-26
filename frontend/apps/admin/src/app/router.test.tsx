import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, createRouter, RouterProvider } from '@tanstack/react-router';
import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { routeTree } from './router';
import { ToastProvider } from '@/components/ui/toast';
import { AuthProvider } from '@/features/auth/auth-context';
import { resetAuthTokenStateForTest } from '@/lib/auth-tokens';

function renderAt(initialPath: string) {
    const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    const testRouter = createRouter({
        routeTree,
        history: createMemoryHistory({ initialEntries: [initialPath] }),
    });
    render(
        <QueryClientProvider client={queryClient}>
            <AuthProvider>
                <ToastProvider>
                    <RouterProvider router={testRouter} />
                </ToastProvider>
            </AuthProvider>
        </QueryClientProvider>,
    );
    return testRouter;
}

describe('Admin router auth guards', () => {
    afterEach(() => {
        resetAuthTokenStateForTest();
        sessionStorage.clear();
    });

    it('redirects unauthenticated / cleanly to /login without loops', async () => {
        const testRouter = renderAt('/');
        await waitFor(() => {
            expect(screen.getByRole('heading', { name: '登录管理后台' })).toBeInTheDocument();
        });
        expect(testRouter.state.location.pathname).toBe('/login');
        expect(testRouter.state.location.searchStr).toBe('');
    });

    it('redirects unauthenticated /posts to /login with redirect=/posts', async () => {
        const testRouter = renderAt('/posts');
        await waitFor(() => {
            expect(screen.getByRole('heading', { name: '登录管理后台' })).toBeInTheDocument();
        });
        expect(testRouter.state.location.pathname).toBe('/login');
        expect(testRouter.state.location.search).toMatchObject({ redirect: '/posts' });
    });

    it('shows expired-session notice on /login?expired=1', async () => {
        renderAt('/login?expired=1');
        await waitFor(() => {
            expect(screen.getByText(/会话已失效，请重新登录/)).toBeInTheDocument();
        });
    });
});
