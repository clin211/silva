import type { Schemas } from '@silva/api-client';
import {
    createRootRoute,
    createRoute,
    createRouter,
    Outlet,
    useLocation,
    useNavigate,
} from '@tanstack/react-router';
import { useEffect, useRef, type ReactNode } from 'react';
import { AdminShell } from '@/components/layout/admin-shell';
import { ForbiddenPage, NotFoundPage } from '@/components/layout/error-pages';
import { AuditLogsPage } from '@/features/audit-logs/audit-logs-page';
import { useAuth } from '@/features/auth/auth-context';
import { LoginPage } from '@/features/auth/login-page';
import { DashboardPage } from '@/features/dashboard/dashboard-page';
import { MediaLibraryPage } from '@/features/media/media-library-page';
import { PostEditorPage } from '@/features/posts/post-editor-page';
import { PostsListPage, type PostsSearchParams } from '@/features/posts/posts-list-page';
import { ProfilePage } from '@/features/profile/profile-page';
import { RolesPage } from '@/features/roles/roles-page';
import {
    SeriesListPage,
    type SeriesSearchParams,
    type SeriesSort,
} from '@/features/series/series-list-page';
import { SeriesWorkbenchPage } from '@/features/series/series-workbench-page';
import { SystemHealthPage } from '@/features/system/system-health-page';
import { TaxonomyPage } from '@/features/taxonomy/taxonomy-page';
import { UsersPage } from '@/features/users/users-page';
import { hasAnyPermission, PERMISSIONS, type PermissionKey } from '@/lib/permissions';

function AuthedLayout() {
    const { status, sessionExpired } = useAuth();
    const location = useLocation();
    const navigate = useNavigate();
    const redirectedRef = useRef(false);

    useEffect(() => {
        if (status === 'authenticated') {
            redirectedRef.current = false;
            return;
        }
        if (status !== 'unauthenticated' || redirectedRef.current) {
            return;
        }
        if (location.pathname.startsWith('/login')) {
            return;
        }
        redirectedRef.current = true;
        const currentTarget = `${location.pathname}${location.searchStr ?? ''}`;
        const safeRedirect = location.pathname === '/' ? undefined : currentTarget;
        void navigate({
            to: '/login',
            search: {
                redirect: safeRedirect,
                expired: sessionExpired ? true : undefined,
            },
            replace: true,
        });
    }, [status, sessionExpired, location.pathname, location.searchStr, navigate]);

    if (status === 'booting') {
        return (
            <div className="grid min-h-screen place-items-center bg-ui-bg text-[14px] text-ui-sub">
                正在验证会话…
            </div>
        );
    }

    if (status === 'unauthenticated') {
        return null;
    }

    return (
        <AdminShell>
            <Outlet />
        </AdminShell>
    );
}

function RequirePermission({
    anyOf,
    children,
}: {
    anyOf: readonly PermissionKey[];
    children: ReactNode;
}) {
    const { user } = useAuth();
    if (!hasAnyPermission(user, anyOf)) {
        return <ForbiddenPage />;
    }
    return <>{children}</>;
}

const rootRoute = createRootRoute({
    component: () => <Outlet />,
    notFoundComponent: () => <AuthedNotFound />,
});

function AuthedNotFound() {
    const { status } = useAuth();
    if (status === 'authenticated') {
        return (
            <AdminShell>
                <NotFoundPage />
            </AdminShell>
        );
    }
    return (
        <div className="min-h-screen bg-ui-bg px-4">
            <NotFoundPage />
        </div>
    );
}

const loginRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/login',
    validateSearch: (
        search: Record<string, unknown>,
    ): { redirect?: string; expired?: boolean } => ({
        redirect:
            typeof search.redirect === 'string' &&
            search.redirect.startsWith('/') &&
            !search.redirect.startsWith('/login')
                ? search.redirect
                : undefined,
        expired:
            search.expired === true ||
            search.expired === 1 ||
            search.expired === '1' ||
            search.expired === 'true'
                ? true
                : undefined,
    }),
    component: LoginPage,
});

const authedRoute = createRoute({
    getParentRoute: () => rootRoute,
    id: '_authed',
    component: AuthedLayout,
});

const dashboardRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/',
    component: DashboardPage,
});

const postsIndexRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/posts',
    validateSearch: (search: Record<string, unknown>): PostsSearchParams => {
        const rawPage = Number(search.page);
        const page = Number.isInteger(rawPage) && rawPage > 0 ? rawPage : undefined;
        const rawPerPage = Number(search.per_page);
        const per_page =
            rawPerPage === 20 || rawPerPage === 50 || rawPerPage === 100
                ? (rawPerPage as 20 | 50 | 100)
                : undefined;
        const scope = search.scope === 'mine' || search.scope === 'all' ? search.scope : undefined;
        const q = typeof search.q === 'string' && search.q.trim() ? search.q : undefined;
        const published =
            search.published === true || search.published === 'true'
                ? true
                : search.published === false || search.published === 'false'
                  ? false
                  : undefined;
        const uncategorized =
            search.uncategorized === true || search.uncategorized === 'true' ? true : undefined;
        const category_id =
            !uncategorized && typeof search.category_id === 'string' && search.category_id
                ? search.category_id
                : undefined;
        const tag_id =
            typeof search.tag_id === 'string' && search.tag_id ? search.tag_id : undefined;
        const in_series =
            search.in_series === true || search.in_series === 'true'
                ? true
                : search.in_series === false || search.in_series === 'false'
                  ? false
                  : undefined;
        const series_id =
            in_series !== false && typeof search.series_id === 'string' && search.series_id
                ? search.series_id
                : undefined;
        const sort =
            search.sort === 'updated_desc' ||
            search.sort === 'created_desc' ||
            search.sort === 'title_asc' ||
            search.sort === 'title_desc'
                ? (search.sort as Schemas['PostSort'])
                : undefined;

        return {
            page,
            per_page,
            scope,
            q,
            published,
            category_id,
            uncategorized,
            tag_id,
            series_id,
            in_series,
            sort,
        };
    },
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_MANAGE]}>
            <PostsListPage />
        </RequirePermission>
    ),
});

const postNewRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/posts/new',
    validateSearch: (search: Record<string, unknown>): { category_id?: string } => {
        const category_id =
            typeof search.category_id === 'string' && search.category_id.trim()
                ? search.category_id.trim()
                : undefined;
        return { category_id };
    },
    component: function PostNewRouteComponent() {
        const { category_id } = postNewRoute.useSearch();
        return (
            <RequirePermission anyOf={[PERMISSIONS.POSTS_CREATE]}>
                <PostEditorPage initialCategoryId={category_id} />
            </RequirePermission>
        );
    },
});

const postEditRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/posts/$postId/edit',
    component: function PostEditRouteComponent() {
        const { postId } = postEditRoute.useParams();
        return (
            <RequirePermission anyOf={[PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_MANAGE]}>
                <PostEditorPage key={postId} postId={postId} />
            </RequirePermission>
        );
    },
});

const seriesIndexRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/series',
    validateSearch: (search: Record<string, unknown>): SeriesSearchParams => {
        const rawPage = Number(search.page);
        const page = Number.isInteger(rawPage) && rawPage > 0 ? rawPage : undefined;
        const rawPerPage = Number(search.per_page);
        const per_page =
            rawPerPage === 20 || rawPerPage === 50 || rawPerPage === 100
                ? (rawPerPage as 20 | 50 | 100)
                : undefined;
        const q = typeof search.q === 'string' && search.q.trim() ? search.q : undefined;
        const category_id =
            typeof search.category_id === 'string' && search.category_id
                ? search.category_id
                : undefined;
        const published =
            search.published === true || search.published === 'true'
                ? true
                : search.published === false || search.published === 'false'
                  ? false
                  : undefined;
        const completion_status =
            search.completion_status === 'ongoing' || search.completion_status === 'completed'
                ? search.completion_status
                : undefined;
        const sort =
            search.sort === 'sort_order_asc' ||
            search.sort === 'updated_desc' ||
            search.sort === 'created_desc'
                ? (search.sort as SeriesSort)
                : undefined;

        return {
            page,
            per_page,
            q,
            category_id,
            published,
            completion_status,
            sort,
        };
    },
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.SERIES_MANAGE]}>
            <SeriesListPage />
        </RequirePermission>
    ),
});

const seriesWorkbenchRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/series/$seriesId',
    component: function SeriesWorkbenchRouteComponent() {
        const { seriesId } = seriesWorkbenchRoute.useParams();
        return (
            <RequirePermission anyOf={[PERMISSIONS.SERIES_MANAGE]}>
                <SeriesWorkbenchPage seriesId={seriesId} />
            </RequirePermission>
        );
    },
});

const taxonomyRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/taxonomy',
    validateSearch: (search: Record<string, unknown>): { tab?: 'categories' | 'tags' } => ({
        tab:
            search.tab === 'tags' ? 'tags' : search.tab === 'categories' ? 'categories' : undefined,
    }),
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.TAXONOMY_MANAGE]}>
            <TaxonomyPage />
        </RequirePermission>
    ),
});

const mediaRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/media',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.MEDIA_MANAGE_OWN, PERMISSIONS.MEDIA_MANAGE]}>
            <MediaLibraryPage />
        </RequirePermission>
    ),
});

const profileRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/profile',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.PROFILE_MANAGE]}>
            <ProfilePage />
        </RequirePermission>
    ),
});

const usersRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/users',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.USERS_MANAGE]}>
            <UsersPage />
        </RequirePermission>
    ),
});

const rolesRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/roles',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.ROLES_READ]}>
            <RolesPage />
        </RequirePermission>
    ),
});

const auditLogsRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/audit-logs',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.AUDIT_READ]}>
            <AuditLogsPage />
        </RequirePermission>
    ),
});

const systemHealthRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/system/health',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.AUDIT_READ, PERMISSIONS.USERS_MANAGE]}>
            <SystemHealthPage />
        </RequirePermission>
    ),
});

const forbiddenRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/403',
    component: ForbiddenPage,
});

export const routeTree = rootRoute.addChildren([
    loginRoute,
    authedRoute.addChildren([
        dashboardRoute,
        postsIndexRoute,
        postNewRoute,
        postEditRoute,
        seriesIndexRoute,
        seriesWorkbenchRoute,
        taxonomyRoute,
        mediaRoute,
        profileRoute,
        usersRoute,
        rolesRoute,
        auditLogsRoute,
        systemHealthRoute,
        forbiddenRoute,
    ]),
]);

export const router = createRouter({
    routeTree,
    defaultPreload: 'intent',
});

declare module '@tanstack/react-router' {
    interface Register {
        router: typeof router;
    }
}
