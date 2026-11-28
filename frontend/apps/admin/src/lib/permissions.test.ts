import type { Schemas } from '@silva/api-client';
import { describe, expect, it } from 'vitest';
import { getPrimaryRoleLabel, hasAnyPermission, hasPermission, PERMISSIONS } from './permissions';

const authorUser: Schemas['UserInfo'] = {
    id: '019535d9-3df7-79fb-b466-fa907fa17f9e',
    email: 'author@example.com',
    display_name: '作者',
    status: 'active',
    roles: ['author'],
    permissions: [
        PERMISSIONS.POSTS_CREATE,
        PERMISSIONS.POSTS_READ_OWN,
        PERMISSIONS.POSTS_EDIT_OWN,
        PERMISSIONS.MEDIA_MANAGE_OWN,
    ],
    created_at: '2026-11-24T12:00:00+08:00',
    updated_at: '2026-11-24T12:00:00+08:00',
    last_login_at: null,
};

describe('permissions RBAC helpers', () => {
    it('evaluates author boundaries accurately', () => {
        expect(hasPermission(authorUser, PERMISSIONS.POSTS_CREATE)).toBe(true);
        expect(hasPermission(authorUser, PERMISSIONS.POSTS_PUBLISH)).toBe(false);
        expect(hasPermission(authorUser, PERMISSIONS.USERS_MANAGE)).toBe(false);
        expect(
            hasAnyPermission(authorUser, [PERMISSIONS.TAXONOMY_MANAGE, PERMISSIONS.AUDIT_READ]),
        ).toBe(false);
        expect(getPrimaryRoleLabel(authorUser.roles)).toBe('Author');
    });

    it('prioritizes highest built-in role label', () => {
        expect(getPrimaryRoleLabel(['author', 'admin'])).toBe('Admin');
        expect(getPrimaryRoleLabel(['author', 'editor'])).toBe('Editor');
    });
});
