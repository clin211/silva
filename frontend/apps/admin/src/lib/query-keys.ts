export const queryKeys = {
    auth: {
        me: ['auth', 'me'] as const,
    },
    posts: {
        all: ['posts'] as const,
        list: (filters: Record<string, unknown>) => ['posts', 'list', filters] as const,
        categoryCounts: (filters: Record<string, unknown>) =>
            ['posts', 'category-counts', filters] as const,
        detail: (id: string) => ['posts', 'detail', id] as const,
    },
    series: {
        all: ['series'] as const,
        list: (filters: Record<string, unknown>) => ['series', 'list', filters] as const,
        detail: (id: string) => ['series', 'detail', id] as const,
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
