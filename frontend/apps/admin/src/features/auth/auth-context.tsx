import type { Schemas } from '@silva/api-client';
import { useQueryClient } from '@tanstack/react-query';
import {
    createContext,
    use,
    useCallback,
    useEffect,
    useMemo,
    useState,
    type ReactNode,
} from 'react';
import { apiClient, unwrapResponse } from '@/lib/api';
import {
    clearTokens,
    getAccessToken,
    getRefreshToken,
    refreshTokensSingleFlight,
    setTokenPair,
    subscribeAuthExpired,
} from '@/lib/auth-tokens';

export type AuthStatus = 'booting' | 'authenticated' | 'unauthenticated';

export interface AuthContextValue {
    status: AuthStatus;
    user: Schemas['UserInfo'] | null;
    sessionExpired: boolean;
    login: (input: Schemas['LoginInput']) => Promise<Schemas['UserInfo']>;
    logout: () => Promise<void>;
    refreshUser: () => Promise<Schemas['UserInfo'] | null>;
    clearExpiredFlag: () => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

async function fetchCurrentUser(): Promise<Schemas['UserInfo']> {
    const res = await unwrapResponse(apiClient.GET('/api/v1/admin/users/me'));
    return res.data;
}

export function AuthProvider({ children }: { children: ReactNode }) {
    const queryClient = useQueryClient();
    const [status, setStatus] = useState<AuthStatus>('booting');
    const [user, setUser] = useState<Schemas['UserInfo'] | null>(null);
    const [sessionExpired, setSessionExpired] = useState(false);

    useEffect(() => {
        const unsubscribe = subscribeAuthExpired(() => {
            setUser(null);
            setStatus('unauthenticated');
            setSessionExpired(true);
            queryClient.clear();
        });
        return unsubscribe;
    }, [queryClient]);

    useEffect(() => {
        let cancelled = false;

        async function bootstrap() {
            const hasAccess = Boolean(getAccessToken());
            const hasRefresh = Boolean(getRefreshToken());

            if (!hasAccess && !hasRefresh) {
                if (!cancelled) {
                    setStatus('unauthenticated');
                }
                return;
            }

            if (!hasAccess && hasRefresh) {
                const token = await refreshTokensSingleFlight();
                if (!token) {
                    if (!cancelled) {
                        setUser(null);
                        setStatus('unauthenticated');
                    }
                    return;
                }
            }

            try {
                const me = await fetchCurrentUser();
                if (!cancelled) {
                    setUser(me);
                    setStatus('authenticated');
                    setSessionExpired(false);
                }
            } catch {
                clearTokens();
                if (!cancelled) {
                    setUser(null);
                    setStatus('unauthenticated');
                }
            }
        }

        void bootstrap();

        return () => {
            cancelled = true;
        };
    }, []);

    const login = useCallback(
        async (input: Schemas['LoginInput']): Promise<Schemas['UserInfo']> => {
            const tokenRes = await unwrapResponse(
                apiClient.POST('/api/v1/admin/sessions', { body: input }),
            );
            setTokenPair(tokenRes.data);
            try {
                const me = await fetchCurrentUser();
                queryClient.clear();
                setUser(me);
                setStatus('authenticated');
                setSessionExpired(false);
                return me;
            } catch (error) {
                clearTokens();
                setUser(null);
                setStatus('unauthenticated');
                throw error;
            }
        },
        [queryClient],
    );

    const logout = useCallback(async (): Promise<void> => {
        try {
            if (getAccessToken()) {
                await apiClient.DELETE('/api/v1/admin/sessions/current');
            }
        } catch {
            // 无论服务端退出是否成功，均清空本地令牌（见前端技术方案 §5）
        } finally {
            clearTokens();
            setUser(null);
            setStatus('unauthenticated');
            setSessionExpired(false);
            queryClient.clear();
        }
    }, [queryClient]);

    const refreshUser = useCallback(async (): Promise<Schemas['UserInfo'] | null> => {
        try {
            const me = await fetchCurrentUser();
            setUser(me);
            setStatus('authenticated');
            return me;
        } catch {
            return null;
        }
    }, []);

    const clearExpiredFlag = useCallback(() => {
        setSessionExpired(false);
    }, []);

    const value = useMemo<AuthContextValue>(
        () => ({
            status,
            user,
            sessionExpired,
            login,
            logout,
            refreshUser,
            clearExpiredFlag,
        }),
        [status, user, sessionExpired, login, logout, refreshUser, clearExpiredFlag],
    );

    return <AuthContext value={value}>{children}</AuthContext>;
}

export function useAuth(): AuthContextValue {
    const ctx = use(AuthContext);
    if (!ctx) {
        throw new Error('useAuth must be used within AuthProvider');
    }
    return ctx;
}
