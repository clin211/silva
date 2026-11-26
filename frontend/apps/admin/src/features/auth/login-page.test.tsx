import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { AuthProvider } from './auth-context';
import { LoginPage } from './login-page';

const mockNavigate = vi.fn();

vi.mock('@tanstack/react-router', async (importOriginal) => {
    const actual = await importOriginal<typeof import('@tanstack/react-router')>();
    return {
        ...actual,
        useNavigate: () => mockNavigate,
        useSearch: () => ({ expired: true, redirect: '/posts' }),
    };
});

describe('LoginPage', () => {
    it('renders expired notice, toggles password visibility, and validates empty inputs', async () => {
        const user = userEvent.setup();
        const queryClient = new QueryClient();

        render(
            <QueryClientProvider client={queryClient}>
                <AuthProvider>
                    <LoginPage />
                </AuthProvider>
            </QueryClientProvider>,
        );

        expect(screen.getByText(/会话已失效，请重新登录/)).toBeInTheDocument();

        const pwInput = screen.getByLabelText('密码');
        expect(pwInput).toHaveAttribute('type', 'password');

        await user.click(screen.getByRole('button', { name: '显示密码' }));
        expect(pwInput).toHaveAttribute('type', 'text');

        await user.click(screen.getByRole('button', { name: '登录' }));
        await waitFor(() => {
            expect(screen.getByText('请输入有效的邮箱地址')).toBeInTheDocument();
            expect(screen.getByText('请输入密码')).toBeInTheDocument();
        });
    });
});
