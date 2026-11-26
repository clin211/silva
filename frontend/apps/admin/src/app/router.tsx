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
import { PostRevisionDetailPage } from '@/features/posts/post-revision-detail-page';
import { PostRevisionsPage } from '@/features/posts/post-revisions-page';
import { PostsListPage, type PostsSearchParams } from '@/features/posts/posts-list-page';
import { ProfilePage } from '@/features/profile/profile-page';
import { PublishingJobDetailPage } from '@/features/publishing/publishing-detail-page';
import { PublishingJobsPage } from '@/features/publishing/publishing-list-page';
import { RolesPage } from '@/features/roles/roles-page';
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
        const scope = search.scope === 'mine' || search.scope === 'all' ? search.scope : undefined;
        const q = typeof search.q === 'string' && search.q.trim() ? search.q : undefined;
        const reviewStatus =
            search.review_status === 'pending' ||
            search.review_status === 'approved' ||
            search.review_status === 'rejected'
                ? (search.review_status as Schemas['ReviewStatus'])
                : undefined;
        const published =
            search.published === true || search.published === 'true'
                ? true
                : search.published === false || search.published === 'false'
                  ? false
                  : undefined;
        const category_id =
            typeof search.category_id === 'string' && search.category_id
                ? search.category_id
                : undefined;
        const tag_id =
            typeof search.tag_id === 'string' && search.tag_id ? search.tag_id : undefined;

        return { page, scope, q, review_status: reviewStatus, published, category_id, tag_id };
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
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.POSTS_CREATE]}>
            <PostEditorPage />
        </RequirePermission>
    ),
});

const postEditRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/posts/$postId/edit',
    component: function PostEditRouteComponent() {
        const { postId } = postEditRoute.useParams();
        return (
            <RequirePermission anyOf={[PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_MANAGE]}>
                <PostEditorPage postId={postId} />
            </RequirePermission>
        );
    },
});

const postRevisionsRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/posts/$postId/revisions',
    component: function PostRevisionsRouteComponent() {
        const { postId } = postRevisionsRoute.useParams();
        return (
            <RequirePermission anyOf={[PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_MANAGE]}>
                <PostRevisionsPage postId={postId} />
            </RequirePermission>
        );
    },
});

const postRevisionDetailRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/posts/$postId/revisions/$revisionId',
    component: function PostRevisionDetailRouteComponent() {
        const { postId, revisionId } = postRevisionDetailRoute.useParams();
        return (
            <RequirePermission anyOf={[PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_MANAGE]}>
                <PostRevisionDetailPage postId={postId} revisionId={revisionId} />
            </RequirePermission>
        );
    },
});

const taxonomyRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/taxonomy',
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

const publishingRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/publishing',
    component: () => (
        <RequirePermission anyOf={[PERMISSIONS.JOBS_READ_OWN, PERMISSIONS.JOBS_READ]}>
            <PublishingJobsPage />
        </RequirePermission>
    ),
});

const publishingDetailRoute = createRoute({
    getParentRoute: () => authedRoute,
    path: '/publishing/$jobId',
    component: function PublishingDetailRouteComponent() {
        const { jobId } = publishingDetailRoute.useParams();
        return (
            <RequirePermission anyOf={[PERMISSIONS.JOBS_READ_OWN, PERMISSIONS.JOBS_READ]}>
                <PublishingJobDetailPage jobId={jobId} />
            </RequirePermission>
        );
    },
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
        postRevisionsRoute,
        postRevisionDetailRoute,
        taxonomyRoute,
        mediaRoute,
        profileRoute,
        publishingRoute,
        publishingDetailRoute,
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
