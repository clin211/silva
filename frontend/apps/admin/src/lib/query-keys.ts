export const queryKeys = {
    auth: {
        me: ['auth', 'me'] as const,
    },
    posts: {
        all: ['posts'] as const,
        list: (filters: Record<string, unknown>) => ['posts', 'list', filters] as const,
        detail: (id: string) => ['posts', 'detail', id] as const,
        revisions: (postId: string, page?: number, perPage?: number) =>
            ['posts', 'detail', postId, 'revisions', { page, perPage }] as const,
        revision: (postId: string, revisionId: string) =>
            ['posts', 'detail', postId, 'revisions', revisionId] as const,
        reviews: (postId: string, page?: number, perPage?: number) =>
            ['posts', 'detail', postId, 'reviews', { page, perPage }] as const,
        review: (postId: string, reviewId: string) =>
            ['posts', 'detail', postId, 'reviews', reviewId] as const,
    },
    taxonomy: {
        categories: ['taxonomy', 'categories'] as const,
        tags: ['taxonomy', 'tags'] as const,
    },
    media: {
        all: ['media'] as const,
        list: (filters: Record<string, unknown>) => ['media', 'list', filters] as const,
        detail: (id: string) => ['media', 'detail', id] as const,
    },
    profile: {
        current: ['profile', 'current'] as const,
    },
    publishing: {
        all: ['publishing'] as const,
        jobs: (filters: Record<string, unknown>) => ['publishing', 'jobs', filters] as const,
        job: (id: string) => ['publishing', 'job', id] as const,
    },
    users: {
        all: ['users'] as const,
        list: (filters: Record<string, unknown>) => ['users', 'list', filters] as const,
        detail: (id: string) => ['users', 'detail', id] as const,
    },
    roles: {
        list: ['roles', 'list'] as const,
    },
    auditLogs: {
        all: ['audit-logs'] as const,
        list: (filters: Record<string, unknown>) => ['audit-logs', 'list', filters] as const,
        detail: (id: string) => ['audit-logs', 'detail', id] as const,
    },
    publicSite: {
        current: ['public', 'site'] as const,
    },
    system: {
        health: ['system', 'health'] as const,
        metrics: ['system', 'metrics'] as const,
    },
} as const;
