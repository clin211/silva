import { websiteUrl } from '@/lib/website';
import { Link, useLocation, useNavigate } from '@tanstack/react-router';
import {
    Activity,
    BookOpen,
    ChevronDown,
    ChevronRight,
    ClipboardList,
    ExternalLink,
    FilePlus2,
    FileText,
    FolderOpen,
    Image as ImageIcon,
    KeyRound,
    Layers,
    LayoutDashboard,
    LogOut,
    Menu,
    PanelLeftClose,
    PanelLeftOpen,
    Search,
    Shield,
    Tag,
    User,
    Users,
    X,
} from 'lucide-react';
import { useEffect, useRef, useState, type ComponentType, type ReactNode } from 'react';
import { CommandPalette } from './command-palette';
import { useAuth } from '@/features/auth/auth-context';
import { ChangePasswordModal } from '@/features/auth/change-password-modal';
import { cn } from '@/lib/cn';
import {
    getPrimaryRoleLabel,
    hasAnyPermission,
    hasPermission,
    PERMISSIONS,
    type UserInfo,
} from '@/lib/permissions';

const SIDEBAR_PREF_KEY = 'silva_admin_sidebar_collapsed';

type TaxonomyTab = 'categories' | 'tags';

type NavLinkTarget =
    | { to: '/' }
    | { to: '/series' }
    | { to: '/posts' }
    | { to: '/taxonomy'; search: { tab: TaxonomyTab } }
    | { to: '/media' }
    | { to: '/users' }
    | { to: '/roles' }
    | { to: '/audit-logs' }
    | { to: '/system/health' };

interface NavLeaf {
    key: string;
    label: string;
    target: NavLinkTarget;
    icon: ComponentType<{ className?: string }>;
}

interface NavParent {
    key: string;
    label: string;
    icon: ComponentType<{ className?: string }>;
    children: NavLeaf[];
}

type NavEntry = NavLeaf | NavParent;

interface NavGroup {
    label: string | null;
    items: NavEntry[];
}

function isNavParent(entry: NavEntry): entry is NavParent {
    return 'children' in entry;
}

function getTaxonomyTab(searchStr: string | undefined): TaxonomyTab {
    const params = new URLSearchParams(searchStr ?? '');
    return params.get('tab') === 'tags' ? 'tags' : 'categories';
}

function buildVisibleNavGroups(user: UserInfo | null | undefined): NavGroup[] {
    const canManageSeries = hasPermission(user, PERMISSIONS.SERIES_MANAGE);
    const canViewPosts = hasAnyPermission(user, [
        PERMISSIONS.POSTS_READ_OWN,
        PERMISSIONS.POSTS_MANAGE,
    ]);
    const canManageTaxonomy = hasPermission(user, PERMISSIONS.TAXONOMY_MANAGE);
    const canManageMedia = hasAnyPermission(user, [
        PERMISSIONS.MEDIA_MANAGE_OWN,
        PERMISSIONS.MEDIA_MANAGE,
    ]);

    const blogChildren: NavLeaf[] = [];
    if (canManageSeries) {
        blogChildren.push({
            key: 'series',
            label: '专栏',
            target: { to: '/series' },
            icon: Layers,
        });
    } else if (canViewPosts) {
        blogChildren.push({
            key: 'posts',
            label: '文章',
            target: { to: '/posts' },
            icon: FileText,
        });
    }
    if (canManageTaxonomy) {
        blogChildren.push(
            {
                key: 'categories',
                label: '分类',
                target: { to: '/taxonomy', search: { tab: 'categories' } },
                icon: FolderOpen,
            },
            {
                key: 'tags',
                label: '标签',
                target: { to: '/taxonomy', search: { tab: 'tags' } },
                icon: Tag,
            },
        );
    }

    const contentItems: NavEntry[] = [];
    if (blogChildren.length > 1) {
        contentItems.push({
            key: 'blog',
            label: '博客管理',
            icon: BookOpen,
            children: blogChildren,
        });
    } else if (blogChildren.length === 1) {
        contentItems.push({
            key: 'blog',
            label: '博客管理',
            target: blogChildren[0]!.target,
            icon: BookOpen,
        });
    }
    if (canManageMedia) {
        contentItems.push({
            key: 'media',
            label: '媒体库',
            target: { to: '/media' },
            icon: ImageIcon,
        });
    }

    const systemItems: NavEntry[] = [];
    if (hasPermission(user, PERMISSIONS.USERS_MANAGE)) {
        systemItems.push({
            key: 'users',
            label: '账户管理',
            target: { to: '/users' },
            icon: Users,
        });
    }
    if (hasPermission(user, PERMISSIONS.ROLES_READ)) {
        systemItems.push({
            key: 'roles',
            label: '角色权限',
            target: { to: '/roles' },
            icon: Shield,
        });
    }
    if (hasPermission(user, PERMISSIONS.AUDIT_READ)) {
        systemItems.push({
            key: 'audit-logs',
            label: '审计日志',
            target: { to: '/audit-logs' },
            icon: ClipboardList,
        });
    }
    if (hasAnyPermission(user, [PERMISSIONS.AUDIT_READ, PERMISSIONS.USERS_MANAGE])) {
        systemItems.push({
            key: 'system-health',
            label: '系统健康',
            target: { to: '/system/health' },
            icon: Activity,
        });
    }

    const groups: NavGroup[] = [
        {
            label: null,
            items: [
                {
                    key: 'dashboard',
                    label: '工作台',
                    target: { to: '/' },
                    icon: LayoutDashboard,
                },
            ],
        },
        { label: '内容管理', items: contentItems },
        { label: '系统与安全', items: systemItems },
    ];

    return groups.filter((group) => group.items.length > 0);
}

interface Crumb {
    label: string;
    to?: string;
}

function buildBreadcrumbs(
    pathname: string,
    searchStr: string | undefined,
    canManageSeries: boolean,
): Crumb[] {
    if (pathname === '/') return [{ label: '工作台' }];
    const blogCrumb: Crumb = { label: '博客管理' };
    const rootCrumb: Crumb = canManageSeries
        ? { label: '专栏', to: '/series' }
        : { label: '文章', to: '/posts' };
    if (pathname === '/series') return [{ label: '内容管理' }, blogCrumb, { label: '专栏' }];
    if (/^\/series\/[^/]+$/.test(pathname)) {
        return [
            { label: '内容管理' },
            blogCrumb,
            { label: '专栏', to: '/series' },
            { label: '专栏工作台' },
        ];
    }
    if (pathname === '/posts') {
        return canManageSeries
            ? [
                  { label: '内容管理' },
                  blogCrumb,
                  { label: '专栏', to: '/series' },
                  { label: '全部文章' },
              ]
            : [{ label: '内容管理' }, blogCrumb, { label: '文章' }];
    }
    if (pathname === '/posts/new') {
        return [{ label: '内容管理' }, blogCrumb, rootCrumb, { label: '新建文章' }];
    }
    if (/^\/posts\/[^/]+\/edit$/.test(pathname)) {
        return [{ label: '内容管理' }, blogCrumb, rootCrumb, { label: '编辑文章' }];
    }
    if (pathname === '/taxonomy') {
        return [
            { label: '内容管理' },
            blogCrumb,
            { label: getTaxonomyTab(searchStr) === 'tags' ? '标签' : '分类' },
        ];
    }
    if (pathname === '/media') return [{ label: '内容管理' }, { label: '媒体库' }];
    if (pathname === '/profile') return [{ label: '个人资料' }];
    if (pathname === '/users') return [{ label: '系统与安全' }, { label: '账户管理' }];
    if (pathname === '/roles') return [{ label: '系统与安全' }, { label: '角色权限' }];
    if (pathname === '/audit-logs') return [{ label: '系统与安全' }, { label: '审计日志' }];
    if (pathname === '/system/health') return [{ label: '系统与安全' }, { label: '系统健康' }];
    if (pathname === '/403') return [{ label: '无权访问' }];
    return [{ label: '页面不存在' }];
}

function isTargetActive(
    pathname: string,
    searchStr: string | undefined,
    target: NavLinkTarget,
): boolean {
    if (target.to === '/') return pathname === '/';
    if (target.to === '/series') {
        return (
            pathname === '/series' ||
            pathname.startsWith('/series/') ||
            pathname === '/posts' ||
            pathname.startsWith('/posts/')
        );
    }
    if (target.to === '/posts') {
        return pathname === '/posts' || pathname.startsWith('/posts/');
    }
    if (target.to === '/taxonomy') {
        return pathname === '/taxonomy' && getTaxonomyTab(searchStr) === target.search.tab;
    }
    return pathname === target.to || pathname.startsWith(`${target.to}/`);
}

function NavTargetLink({
    target,
    title,
    onClick,
    ariaCurrent,
    className,
    children,
}: {
    target: NavLinkTarget;
    title?: string;
    onClick?: () => void;
    ariaCurrent?: 'page';
    className: string;
    children: ReactNode;
}) {
    if (target.to === '/taxonomy') {
        return (
            <Link
                to="/taxonomy"
                search={target.search}
                title={title}
                onClick={onClick}
                aria-current={ariaCurrent}
                className={className}
            >
                {children}
            </Link>
        );
    }
    return (
        <Link
            to={target.to}
            title={title}
            onClick={onClick}
            aria-current={ariaCurrent}
            className={className}
        >
            {children}
        </Link>
    );
}

function readInitialCollapsed(): boolean {
    try {
        return globalThis.localStorage?.getItem(SIDEBAR_PREF_KEY) === '1';
    } catch {
        return false;
    }
}

export function SilvaBrandMark({ className = 'h-7 w-7' }: { className?: string }) {
    return (
        <span
            className={cn(
                'grid shrink-0 place-items-center rounded-[9px] bg-gradient-to-br from-[#1E925E] to-[#0E6A41] text-white shadow-[0_2px_6px_rgba(14,106,65,0.28),inset_0_1px_0_rgba(255,255,255,0.28)]',
                className,
            )}
        >
            <svg
                viewBox="0 0 24 24"
                fill="none"
                className="h-4 w-4"
                stroke="currentColor"
                strokeWidth="2.1"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
            >
                <circle cx="12" cy="12" r="8.5" strokeOpacity="0.45" />
                <path d="M15 9.2c-.8-.9-2-1.4-3.3-1.4-2.1 0-3.5 1.1-3.5 2.5 0 1.6 1.6 2 3.6 2.4 2 .4 3.4 1 3.4 2.6 0 1.6-1.5 2.7-3.7 2.7-1.5 0-2.9-.6-3.7-1.5" />
            </svg>
        </span>
    );
}

export function AdminShell({ children }: { children: ReactNode }) {
    const { user, logout } = useAuth();
    const location = useLocation();
    const navigate = useNavigate();
    const [collapsed, setCollapsed] = useState(readInitialCollapsed);
    const [openSubmenus, setOpenSubmenus] = useState<Record<string, boolean>>({ blog: true });
    const [compactFlyout, setCompactFlyout] = useState<{
        key: string;
        top: number;
        left: number;
    } | null>(null);
    const [drawerOpen, setDrawerOpen] = useState(false);
    const [menuOpen, setMenuOpen] = useState(false);
    const [cmdOpen, setCmdOpen] = useState(false);
    const [pwModalOpen, setPwModalOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);
    const flyoutRef = useRef<HTMLDivElement>(null);
    const flyoutTriggerRef = useRef<HTMLButtonElement | null>(null);

    // Global ⌘K / Ctrl+K shortcut for Command Palette
    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
                event.preventDefault();
                setCmdOpen((prev) => !prev);
            }
        };
        document.addEventListener('keydown', onKeyDown);
        return () => document.removeEventListener('keydown', onKeyDown);
    }, []);

    useEffect(() => {
        if (!menuOpen) return;
        const onPointerDown = (event: MouseEvent) => {
            if (!menuRef.current?.contains(event.target as Node)) {
                setMenuOpen(false);
            }
        };
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') setMenuOpen(false);
        };
        document.addEventListener('mousedown', onPointerDown);
        document.addEventListener('keydown', onKeyDown);
        return () => {
            document.removeEventListener('mousedown', onPointerDown);
            document.removeEventListener('keydown', onKeyDown);
        };
    }, [menuOpen]);

    useEffect(() => {
        if (!compactFlyout) return;
        const onPointerDown = (event: MouseEvent) => {
            const target = event.target as Node;
            if (
                !flyoutRef.current?.contains(target) &&
                !flyoutTriggerRef.current?.contains(target)
            ) {
                setCompactFlyout(null);
            }
        };
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') setCompactFlyout(null);
        };
        document.addEventListener('mousedown', onPointerDown);
        document.addEventListener('keydown', onKeyDown);
        return () => {
            document.removeEventListener('mousedown', onPointerDown);
            document.removeEventListener('keydown', onKeyDown);
        };
    }, [compactFlyout]);

    const toggleCollapsed = () => {
        setCompactFlyout(null);
        setCollapsed((prev) => {
            const next = !prev;
            try {
                globalThis.localStorage?.setItem(SIDEBAR_PREF_KEY, next ? '1' : '0');
            } catch {
                // Ignore storage quota / private mode failures
            }
            return next;
        });
    };

    const canManageSeries = hasPermission(user, PERMISSIONS.SERIES_MANAGE);
    const visibleGroups = buildVisibleNavGroups(user);
    const activeFlyoutParent = compactFlyout
        ? visibleGroups
              .flatMap((group) => group.items)
              .find(
                  (item): item is NavParent => isNavParent(item) && item.key === compactFlyout.key,
              )
        : undefined;

    const crumbs = buildBreadcrumbs(location.pathname, location.searchStr, canManageSeries);
    const roleLabel = getPrimaryRoleLabel(user?.roles);
    const avatarInitial = user?.display_name?.trim().slice(0, 1).toUpperCase() || 'S';
    const canCreatePost = hasPermission(user, PERMISSIONS.POSTS_CREATE);
    const isEditorRoute =
        location.pathname === '/posts/new' || /^\/posts\/[^/]+\/edit$/.test(location.pathname);

    const renderNavList = (isCompact: boolean) => (
        <nav className="flex-1 overflow-y-auto px-3 py-3" aria-label="后台导航">
            {visibleGroups.map((group, idx) => (
                <div
                    key={group.label ?? `root-${idx}`}
                    className={
                        idx === 0 ? '' : isCompact ? 'mt-4 border-t border-ui-line pt-4' : 'mt-5'
                    }
                >
                    {group.label && !isCompact ? (
                        <div className="mb-1.5 px-3 text-[11px] font-medium tracking-[0.06em] text-[#8E9C96] uppercase">
                            {group.label}
                        </div>
                    ) : null}
                    <ul className="space-y-1">
                        {group.items.map((item) => {
                            const Icon = item.icon;
                            if (isNavParent(item)) {
                                const hasActiveChild = item.children.some((child) =>
                                    isTargetActive(
                                        location.pathname,
                                        location.searchStr,
                                        child.target,
                                    ),
                                );
                                const isExpanded = openSubmenus[item.key] ?? true;
                                const isFlyoutOpen = compactFlyout?.key === item.key;
                                const parentPillActive =
                                    hasActiveChild && (isCompact || !isExpanded);

                                return (
                                    <li key={item.key}>
                                        <button
                                            type="button"
                                            title={isCompact ? item.label : undefined}
                                            aria-label={isCompact ? item.label : undefined}
                                            aria-expanded={isCompact ? isFlyoutOpen : isExpanded}
                                            aria-haspopup={isCompact ? 'menu' : undefined}
                                            onClick={(event) => {
                                                if (isCompact) {
                                                    const btn = event.currentTarget;
                                                    flyoutTriggerRef.current = btn;
                                                    if (isFlyoutOpen) {
                                                        setCompactFlyout(null);
                                                        return;
                                                    }
                                                    const rect = btn.getBoundingClientRect();
                                                    setCompactFlyout({
                                                        key: item.key,
                                                        top: rect.top,
                                                        left: rect.right + 10,
                                                    });
                                                    return;
                                                }
                                                setOpenSubmenus((prev) => ({
                                                    ...prev,
                                                    [item.key]: !(prev[item.key] ?? true),
                                                }));
                                            }}
                                            className={cn(
                                                'group relative flex w-full items-center rounded-[11px] transition-all duration-150',
                                                isCompact
                                                    ? 'justify-center p-2.5'
                                                    : 'gap-2.5 px-3 py-2.5 text-[13.5px]',
                                                parentPillActive
                                                    ? 'nav-active-pill font-medium'
                                                    : hasActiveChild
                                                      ? 'font-medium text-[#111917] hover:bg-[#F3F6F5]'
                                                      : 'text-[#4B5A54] hover:bg-[#F3F6F5] hover:text-[#111917]',
                                            )}
                                        >
                                            <Icon
                                                className={cn(
                                                    'h-[17px] w-[17px] shrink-0 transition-colors',
                                                    parentPillActive
                                                        ? 'text-white'
                                                        : hasActiveChild
                                                          ? 'text-accent'
                                                          : 'text-[#7A8A84] group-hover:text-[#111917]',
                                                )}
                                            />
                                            {!isCompact ? (
                                                <>
                                                    <span className="flex-1 truncate text-left">
                                                        {item.label}
                                                    </span>
                                                    <ChevronDown
                                                        aria-hidden="true"
                                                        className={cn(
                                                            'h-3.5 w-3.5 shrink-0 transition-transform duration-150',
                                                            parentPillActive
                                                                ? 'text-white/80'
                                                                : 'text-[#8E9C96]',
                                                            !isExpanded && '-rotate-90',
                                                        )}
                                                    />
                                                </>
                                            ) : null}
                                        </button>
                                        {!isCompact && isExpanded ? (
                                            <ul
                                                aria-label={`${item.label}子菜单`}
                                                className="mt-1 ml-5 space-y-0.5 border-l border-[#E6ECE9] pl-2.5"
                                            >
                                                {item.children.map((child) => {
                                                    const childActive = isTargetActive(
                                                        location.pathname,
                                                        location.searchStr,
                                                        child.target,
                                                    );
                                                    const ChildIcon = child.icon;
                                                    return (
                                                        <li key={child.key}>
                                                            <NavTargetLink
                                                                target={child.target}
                                                                onClick={() => setDrawerOpen(false)}
                                                                ariaCurrent={
                                                                    childActive ? 'page' : undefined
                                                                }
                                                                className={cn(
                                                                    'group relative flex items-center gap-2 rounded-[10px] px-2.5 py-2 text-[13px] transition-all duration-150',
                                                                    childActive
                                                                        ? 'nav-active-pill font-medium'
                                                                        : 'text-[#4B5A54] hover:bg-[#F3F6F5] hover:text-[#111917]',
                                                                )}
                                                            >
                                                                <ChildIcon
                                                                    className={cn(
                                                                        'h-[15px] w-[15px] shrink-0 transition-colors',
                                                                        childActive
                                                                            ? 'text-white'
                                                                            : 'text-[#8A9993] group-hover:text-[#111917]',
                                                                    )}
                                                                />
                                                                <span className="truncate">
                                                                    {child.label}
                                                                </span>
                                                            </NavTargetLink>
                                                        </li>
                                                    );
                                                })}
                                            </ul>
                                        ) : null}
                                    </li>
                                );
                            }

                            const active = isTargetActive(
                                location.pathname,
                                location.searchStr,
                                item.target,
                            );
                            return (
                                <li key={item.key}>
                                    <NavTargetLink
                                        target={item.target}
                                        title={isCompact ? item.label : undefined}
                                        onClick={() => {
                                            setDrawerOpen(false);
                                            setCompactFlyout(null);
                                        }}
                                        ariaCurrent={active ? 'page' : undefined}
                                        className={cn(
                                            'group relative flex items-center rounded-[11px] transition-all duration-150',
                                            isCompact
                                                ? 'justify-center p-2.5'
                                                : 'gap-2.5 px-3 py-2.5 text-[13.5px]',
                                            active
                                                ? 'nav-active-pill font-medium'
                                                : 'text-[#4B5A54] hover:bg-[#F3F6F5] hover:text-[#111917]',
                                        )}
                                    >
                                        <Icon
                                            className={cn(
                                                'h-[17px] w-[17px] shrink-0 transition-colors',
                                                active
                                                    ? 'text-white'
                                                    : 'text-[#7A8A84] group-hover:text-[#111917]',
                                            )}
                                        />
                                        {!isCompact ? (
                                            <span className="truncate">{item.label}</span>
                                        ) : null}
                                    </NavTargetLink>
                                </li>
                            );
                        })}
                    </ul>
                </div>
            ))}
        </nav>
    );

    return (
        <div
            className={cn(
                'min-h-screen transition-[grid-template-columns] duration-200 lg:grid',
                collapsed
                    ? 'lg:grid-cols-[72px_minmax(0,1fr)]'
                    : 'lg:grid-cols-[248px_minmax(0,1fr)]',
            )}
        >
            <a
                href="#admin-main"
                className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[100] focus:rounded-lg focus:bg-white focus:px-3 focus:py-2 focus:shadow-lg"
            >
                跳到工作区
            </a>

            {/* Desktop Crisp White Light Sidebar */}
            <aside className="sticky top-0 hidden h-screen flex-col border-r border-ui-line bg-white text-ui-text lg:flex">
                <div
                    className={cn(
                        'flex h-[62px] shrink-0 items-center',
                        collapsed ? 'justify-center px-2' : 'justify-between px-4',
                    )}
                >
                    <Link
                        to="/"
                        className="flex min-w-0 items-center gap-2.5"
                        title={collapsed ? 'Silva Studio' : undefined}
                    >
                        <SilvaBrandMark className="h-8 w-8" />
                        {!collapsed ? (
                            <span className="truncate text-[17px] font-semibold tracking-[-0.02em] text-ui-text">
                                Silva
                            </span>
                        ) : null}
                    </Link>

                    {!collapsed ? (
                        <button
                            type="button"
                            onClick={toggleCollapsed}
                            title="收起侧栏"
                            aria-label="收起侧栏"
                            className="rounded-lg p-1.5 text-[#8A9993] transition-colors hover:bg-[#F3F6F5] hover:text-ui-text"
                        >
                            <PanelLeftClose className="h-4 w-4" />
                        </button>
                    ) : null}
                </div>

                {/* Quick Command Trigger inside sidebar */}
                {!collapsed ? (
                    <div className="px-3 pb-1">
                        <button
                            type="button"
                            onClick={() => setCmdOpen(true)}
                            className="flex h-9 w-full items-center gap-2 rounded-[10px] border border-[#E6ECE9] bg-[#F5F7F6] px-3 text-[13px] text-[#7D8C86] transition-colors hover:border-[#CDD9D3] hover:bg-[#EEF2F0] hover:text-ui-text"
                        >
                            <Search className="h-3.5 w-3.5 shrink-0" />
                            <span className="flex-1 text-left">快速跳转…</span>
                            <kbd className="rounded-md border border-[#E2E9E6] bg-white px-1.5 py-0.5 font-code text-[10.5px] text-[#7D8C86] shadow-2xs">
                                ⌘K
                            </kbd>
                        </button>
                    </div>
                ) : (
                    <div className="flex flex-col items-center gap-1 px-2 pb-1">
                        <button
                            type="button"
                            onClick={toggleCollapsed}
                            title="展开侧栏"
                            aria-label="展开侧栏"
                            className="grid h-9 w-9 place-items-center rounded-[10px] text-[#7A8A84] transition-colors hover:bg-[#F3F6F5] hover:text-ui-text"
                        >
                            <PanelLeftOpen className="h-4 w-4" />
                        </button>
                        <button
                            type="button"
                            onClick={() => setCmdOpen(true)}
                            title="快速跳转 (⌘K)"
                            aria-label="快速跳转"
                            className="grid h-9 w-9 place-items-center rounded-[10px] text-[#7A8A84] transition-colors hover:bg-[#F3F6F5] hover:text-ui-text"
                        >
                            <Search className="h-4 w-4" />
                        </button>
                    </div>
                )}

                {renderNavList(collapsed)}
            </aside>

            {/* Mobile navigation drawer */}
            {drawerOpen ? (
                <div className="fixed inset-0 z-50 lg:hidden">
                    <div
                        aria-hidden="true"
                        onClick={() => setDrawerOpen(false)}
                        className="animate-fade-in fixed inset-0 bg-[#111917]/35 backdrop-blur-[2px]"
                    />
                    <dialog
                        open
                        aria-modal="true"
                        aria-label="后台导航"
                        className="animate-drawer-left relative z-10 m-0 flex h-full w-[276px] max-w-[85vw] flex-col border-0 bg-white p-0 text-ui-text shadow-2xl"
                    >
                        <div className="flex h-[62px] items-center justify-between border-b border-ui-line px-4">
                            <Link
                                to="/"
                                onClick={() => setDrawerOpen(false)}
                                className="flex items-center gap-2.5"
                            >
                                <SilvaBrandMark className="h-8 w-8" />
                                <span className="text-[17px] font-semibold tracking-[-0.02em] text-ui-text">
                                    Silva
                                </span>
                            </Link>
                            <button
                                type="button"
                                onClick={() => setDrawerOpen(false)}
                                aria-label="关闭导航"
                                className="rounded-lg p-1.5 text-ui-sub hover:bg-[#F3F6F5] hover:text-ui-text"
                            >
                                <X className="h-4 w-4" />
                            </button>
                        </div>
                        {renderNavList(false)}
                    </dialog>
                </div>
            ) : null}

            {/* Main Workspace Column */}
            <div className="flex min-w-0 flex-col bg-ui-bg">
                <header className="sticky top-0 z-30 flex h-[62px] items-center gap-3 border-b border-ui-line bg-white/95 px-4 backdrop-blur-md md:px-7">
                    <button
                        type="button"
                        onClick={() => setDrawerOpen(true)}
                        aria-label="打开导航"
                        className="-ml-1 rounded-lg p-2 text-ui-text hover:bg-[#E4ECEA] lg:hidden"
                    >
                        <Menu className="h-5 w-5" />
                    </button>

                    <nav aria-label="面包屑" className="min-w-0 flex-1">
                        <ol className="flex items-center gap-1 truncate text-[13px] text-ui-sub">
                            <li className="hidden items-center gap-1 sm:flex">
                                <Link
                                    to="/"
                                    className="rounded px-1 py-0.5 text-[#8A9993] transition-colors hover:bg-[#F3F6F5] hover:text-ui-text"
                                >
                                    Silva
                                </Link>
                                <ChevronRight
                                    aria-hidden="true"
                                    className="h-3.5 w-3.5 shrink-0 text-[#B4C2BC]"
                                />
                            </li>
                            {crumbs.map((crumb, index) => {
                                const isLast = index === crumbs.length - 1;
                                return (
                                    <li
                                        key={`${crumb.label}-${index}`}
                                        className="flex min-w-0 items-center gap-1"
                                    >
                                        {index > 0 ? (
                                            <ChevronRight
                                                aria-hidden="true"
                                                className="h-3.5 w-3.5 shrink-0 text-[#B4C2BC]"
                                            />
                                        ) : null}
                                        {crumb.to && !isLast ? (
                                            <Link
                                                to={crumb.to}
                                                className="truncate rounded px-1 py-0.5 transition-colors hover:bg-[#F3F6F5] hover:text-ui-text"
                                            >
                                                {crumb.label}
                                            </Link>
                                        ) : (
                                            <span
                                                aria-current={isLast ? 'page' : undefined}
                                                className={cn(
                                                    'truncate px-1',
                                                    isLast ? 'font-medium text-ui-text' : '',
                                                )}
                                            >
                                                {crumb.label}
                                            </span>
                                        )}
                                    </li>
                                );
                            })}
                        </ol>
                    </nav>

                    <div className="flex items-center gap-2.5">
                        <a
                            href={websiteUrl}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="hidden h-9 items-center gap-1.5 rounded-[10px] border border-ui-line bg-white px-3 text-[13px] font-medium text-ui-sub shadow-2xs transition-colors hover:border-ui-line-strong hover:text-ui-text sm:inline-flex"
                        >
                            <span>查看站点</span>
                            <ExternalLink className="h-3.5 w-3.5" />
                        </a>

                        <div ref={menuRef} className="relative">
                            <button
                                type="button"
                                onClick={() => setMenuOpen((prev) => !prev)}
                                aria-expanded={menuOpen}
                                aria-haspopup="menu"
                                className="flex h-9 items-center gap-2 rounded-full border border-ui-line bg-white p-1 shadow-2xs transition-colors hover:border-ui-line-strong sm:py-1 sm:pr-2.5 sm:pl-1"
                            >
                                <span className="grid h-7 w-7 place-items-center rounded-full bg-accent-tint text-[12px] font-semibold text-accent-2 ring-1 ring-accent/15">
                                    {avatarInitial}
                                </span>
                                <span className="hidden flex-col items-start leading-tight sm:flex">
                                    <span className="max-w-[110px] truncate text-[12.5px] font-medium text-ui-text">
                                        {user?.display_name ?? '账户'}
                                    </span>
                                    <span className="text-[10.5px] text-ui-sub">{roleLabel}</span>
                                </span>
                                <ChevronDown className="hidden h-3.5 w-3.5 text-ui-sub sm:block" />
                            </button>

                            {menuOpen ? (
                                <div
                                    role="menu"
                                    className="animate-modal-in absolute right-0 mt-2 w-60 overflow-hidden rounded-xl border border-ui-line bg-white py-1 text-[13.5px] shadow-[var(--shadow-overlay)]"
                                >
                                    <div className="border-b border-ui-line bg-ui-subtle px-3.5 py-2.5">
                                        <div className="truncate font-semibold text-ui-text">
                                            {user?.display_name}
                                        </div>
                                        <div className="mt-0.5 truncate text-[12px] text-ui-sub">
                                            {user?.email}
                                        </div>
                                        <span className="mt-1.5 inline-flex rounded-full bg-accent-tint px-2 py-0.5 text-[11px] font-medium text-accent-2">
                                            {roleLabel}
                                        </span>
                                    </div>
                                    <div className="p-1">
                                        {hasAnyPermission(user, [PERMISSIONS.PROFILE_MANAGE]) ? (
                                            <Link
                                                to="/profile"
                                                role="menuitem"
                                                onClick={() => setMenuOpen(false)}
                                                className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-ui-text transition-colors hover:bg-ui-bg"
                                            >
                                                <User className="h-4 w-4 text-ui-sub" />
                                                个人资料
                                            </Link>
                                        ) : null}
                                        <button
                                            type="button"
                                            role="menuitem"
                                            onClick={() => {
                                                setMenuOpen(false);
                                                setPwModalOpen(true);
                                            }}
                                            className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-ui-text transition-colors hover:bg-ui-bg"
                                        >
                                            <KeyRound className="h-4 w-4 text-ui-sub" />
                                            修改密码
                                        </button>
                                    </div>
                                    <div className="border-t border-ui-line p-1">
                                        <button
                                            type="button"
                                            role="menuitem"
                                            onClick={async () => {
                                                setMenuOpen(false);
                                                await logout();
                                                await navigate({ to: '/login' });
                                            }}
                                            className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-bad transition-colors hover:bg-bad-tint"
                                        >
                                            <LogOut className="h-4 w-4" />
                                            退出登录
                                        </button>
                                    </div>
                                </div>
                            ) : null}
                        </div>

                        {canCreatePost && !isEditorRoute ? (
                            <Link
                                to="/posts/new"
                                className="hidden h-9 items-center gap-1.5 rounded-[10px] bg-accent px-3.5 text-[13px] font-medium text-white shadow-[0_1px_2px_rgba(19,122,77,0.28)] transition-colors hover:bg-accent-2 md:inline-flex"
                            >
                                <FilePlus2 className="h-3.5 w-3.5" />
                                写文章
                            </Link>
                        ) : null}
                    </div>
                </header>

                <main
                    id="admin-main"
                    className={cn(
                        'min-w-0 flex-1',
                        isEditorRoute ? 'px-4 py-4 pb-10 md:px-5' : 'px-4 py-7 pb-20 md:px-8',
                    )}
                >
                    {children}
                </main>
            </div>

            {collapsed && compactFlyout && activeFlyoutParent ? (
                <div
                    ref={flyoutRef}
                    role="menu"
                    aria-label={activeFlyoutParent.label}
                    style={{ top: compactFlyout.top, left: compactFlyout.left }}
                    className="animate-modal-in fixed z-50 w-44 rounded-xl border border-ui-line bg-white p-1.5 shadow-[var(--shadow-overlay)]"
                >
                    <div className="px-2.5 py-1.5 text-[11px] font-medium tracking-[0.04em] text-[#8E9C96]">
                        {activeFlyoutParent.label}
                    </div>
                    <ul className="space-y-0.5">
                        {activeFlyoutParent.children.map((child) => {
                            const childActive = isTargetActive(
                                location.pathname,
                                location.searchStr,
                                child.target,
                            );
                            const ChildIcon = child.icon;
                            return (
                                <li key={child.key}>
                                    <NavTargetLink
                                        target={child.target}
                                        onClick={() => setCompactFlyout(null)}
                                        ariaCurrent={childActive ? 'page' : undefined}
                                        className={cn(
                                            'flex items-center gap-2 rounded-lg px-2.5 py-2 text-[13px] transition-colors',
                                            childActive
                                                ? 'nav-active-pill font-medium'
                                                : 'text-[#4B5A54] hover:bg-[#F3F6F5] hover:text-[#111917]',
                                        )}
                                    >
                                        <ChildIcon
                                            className={cn(
                                                'h-[15px] w-[15px] shrink-0',
                                                childActive ? 'text-white' : 'text-[#8A9993]',
                                            )}
                                        />
                                        <span className="truncate">{child.label}</span>
                                    </NavTargetLink>
                                </li>
                            );
                        })}
                    </ul>
                </div>
            ) : null}

            <CommandPalette
                open={cmdOpen}
                onClose={() => setCmdOpen(false)}
                onOpenChangePassword={() => setPwModalOpen(true)}
                websiteUrl={websiteUrl}
            />
            <ChangePasswordModal open={pwModalOpen} onClose={() => setPwModalOpen(false)} />
        </div>
    );
}
