import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { router } from './router';
import { ToastProvider } from '@/components/ui/toast';
import { AuthProvider } from '@/features/auth/auth-context';
import { isApiError } from '@/lib/errors';

const queryClient = new QueryClient({
    defaultOptions: {
        queries: {
            staleTime: 15_000,
            refetchOnWindowFocus: false,
            retry: (failureCount, error) => {
                if (isApiError(error) && error.status >= 400 && error.status < 500) {
                    return false;
                }
                return failureCount < 1;
            },
        },
        mutations: {
            retry: false,
        },
    },
});

export function App() {
    return (
        <QueryClientProvider client={queryClient}>
            <AuthProvider>
                <ToastProvider>
                    <RouterProvider router={router} />
                </ToastProvider>
            </AuthProvider>
        </QueryClientProvider>
    );
}
