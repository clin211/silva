import { useNavigate } from '@tanstack/react-router';
import {
    Activity,
    ClipboardList,
    CornerDownLeft,
    ExternalLink,
    FilePlus2,
    FileText,
    Image as ImageIcon,
    KeyRound,
    LayoutDashboard,
    Search,
    Send,
    Shield,
    Tag,
    User,
    Users,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState, type ComponentType } from 'react';
import { useAuth } from '@/features/auth/auth-context';
import { cn } from '@/lib/cn';
import { hasAnyPermission, PERMISSIONS, type PermissionKey } from '@/lib/permissions';

interface CommandAction {
    id: string;
    group: string;
    label: string;
    hint: string;
    keywords: string;
    icon: ComponentType<{ className?: string }>;
    permissions?: readonly PermissionKey[];
    run: () => void;
}

export function CommandPalette({
    open,
    onClose,
    onOpenChangePassword,
    websiteUrl,
}: {
    open: boolean;
    onClose: () => void;
    onOpenChangePassword: () => void;
    websiteUrl: string;
}) {
    const { user } = useAuth();
    const navigate = useNavigate();
    const inputRef = useRef<HTMLInputElement>(null);
    const [query, setQuery] = useState('');
    const [activeIndex, setActiveIndex] = useState(0);

    const actions = useMemo<CommandAction[]>(
        () => [
            {
                id: 'new-post',
                group: '快捷操作',
                label: '新建文章',
                hint: '打开 Markdown 编辑器',
                keywords: 'new post write create 新建 写作 文章',
                icon: FilePlus2,
                permissions: [PERMISSIONS.POSTS_CREATE],
                run: () => void navigate({ to: '/posts/new' }),
            },
            {
                id: 'change-pw',
                group: '快捷操作',
                label: '修改当前账户密码',
                hint: '安全设置',
                keywords: 'password security 密码 修改',
                icon: KeyRound,
                run: () => onOpenChangePassword(),
            },
            {
                id: 'open-site',
                group: '快捷操作',
                label: '打开公开站点',
                hint: websiteUrl,
                keywords: 'website public site 公开 站点 前台',
                icon: ExternalLink,
                run: () => window.open(websiteUrl, '_blank', 'noopener,noreferrer'),
            },
            {
                id: 'nav-dashboard',
                group: '页面导航',
                label: '工作台',
                hint: '/',
                keywords: 'dashboard home 工作台 首页',
                icon: LayoutDashboard,
                run: () => void navigate({ to: '/' }),
            },
            {
                id: 'nav-posts',
                group: '页面导航',
                label: '文章列表',
                hint: '/posts',
                keywords: 'posts articles 文章 审核 发布',
                icon: FileText,
                permissions: [PERMISSIONS.POSTS_READ_OWN, PERMISSIONS.POSTS_MANAGE],
                run: () => void navigate({ to: '/posts' }),
            },
            {
                id: 'nav-taxonomy',
                group: '页面导航',
                label: '分类与标签',
                hint: '/taxonomy',
                keywords: 'taxonomy categories tags 分类 标签',
                icon: Tag,
                permissions: [PERMISSIONS.TAXONOMY_MANAGE],
                run: () => void navigate({ to: '/taxonomy' }),
            },
            {
                id: 'nav-media',
                group: '页面导航',
                label: '媒体库',
                hint: '/media',
                keywords: 'media images upload 媒体 图片 上传',
                icon: ImageIcon,
                permissions: [PERMISSIONS.MEDIA_MANAGE_OWN, PERMISSIONS.MEDIA_MANAGE],
                run: () => void navigate({ to: '/media' }),
            },
            {
                id: 'nav-profile',
                group: '页面导航',
                label: '个人资料',
                hint: '/profile',
                keywords: 'profile about 个人 资料 关于',
                icon: User,
                permissions: [PERMISSIONS.PROFILE_MANAGE],
                run: () => void navigate({ to: '/profile' }),
            },
            {
                id: 'nav-publishing',
                group: '页面导航',
                label: '站点发布任务',
                hint: '/publishing',
                keywords: 'publishing jobs build generation 发布 构建',
                icon: Send,
                permissions: [PERMISSIONS.JOBS_READ_OWN, PERMISSIONS.JOBS_READ],
                run: () => void navigate({ to: '/publishing' }),
            },
            {
                id: 'nav-users',
                group: '页面导航',
                label: '账户管理',
                hint: '/users',
                keywords: 'users accounts 账户 成员',
                icon: Users,
                permissions: [PERMISSIONS.USERS_MANAGE],
                run: () => void navigate({ to: '/users' }),
            },
            {
                id: 'nav-roles',
                group: '页面导航',
                label: '角色与权限矩阵',
                hint: '/roles',
                keywords: 'roles permissions rbac 角色 权限',
                icon: Shield,
                permissions: [PERMISSIONS.ROLES_READ],
                run: () => void navigate({ to: '/roles' }),
            },
            {
                id: 'nav-audit',
                group: '页面导航',
                label: '审计日志',
                hint: '/audit-logs',
                keywords: 'audit logs security 审计 日志',
                icon: ClipboardList,
                permissions: [PERMISSIONS.AUDIT_READ],
                run: () => void navigate({ to: '/audit-logs' }),
            },
            {
                id: 'nav-health',
                group: '页面导航',
                label: '系统健康与指标',
                hint: '/system/health',
                keywords: 'health metrics prometheus 系统 健康 监控',
                icon: Activity,
                permissions: [PERMISSIONS.AUDIT_READ, PERMISSIONS.USERS_MANAGE],
                run: () => void navigate({ to: '/system/health' }),
            },
        ],
        [navigate, onOpenChangePassword, websiteUrl],
    );

    const filtered = useMemo(() => {
        const q = query.trim().toLowerCase();
        return actions.filter((item) => {
            if (item.permissions && !hasAnyPermission(user, item.permissions)) {
                return false;
            }
            if (!q) return true;
            return (
                item.label.toLowerCase().includes(q) ||
                item.hint.toLowerCase().includes(q) ||
                item.keywords.toLowerCase().includes(q)
            );
        });
    }, [actions, query, user]);

    useEffect(() => {
        if (!open) return;
        inputRef.current?.focus();
    }, [open]);

    const safeIndex = filtered.length > 0 ? Math.min(activeIndex, filtered.length - 1) : 0;

    const executeItem = (item: CommandAction | undefined) => {
        if (!item) return;
        onClose();
        setQuery('');
        setActiveIndex(0);
        item.run();
    };

    if (!open) return null;

    return (
        <div className="fixed inset-0 z-[70] flex items-start justify-center px-4 pt-[12vh]">
            <div
                aria-hidden="true"
                onClick={() => {
                    onClose();
                    setQuery('');
                }}
                className="animate-fade-in fixed inset-0 bg-[#091C1A]/50 backdrop-blur-[2px]"
            />
            <dialog
                open
                aria-modal="true"
                aria-label="快捷指令面板"
                className="animate-modal-in relative z-10 m-0 w-full max-w-[540px] overflow-hidden rounded-2xl border border-ui-line bg-white p-0 text-ui-text shadow-[var(--shadow-overlay)]"
            >
                <div className="flex items-center gap-3 border-b border-ui-line px-4 py-3.5">
                    <Search className="h-4 w-4 shrink-0 text-ui-sub" aria-hidden="true" />
                    <input
                        ref={inputRef}
                        type="text"
                        value={query}
                        onChange={(event) => {
                            setQuery(event.target.value);
                            setActiveIndex(0);
                        }}
                        onKeyDown={(event) => {
                            if (event.key === 'Escape') {
                                event.preventDefault();
                                onClose();
                                setQuery('');
                            } else if (event.key === 'ArrowDown') {
                                event.preventDefault();
                                setActiveIndex((prev) =>
                                    filtered.length > 0 ? (prev + 1) % filtered.length : 0,
                                );
                            } else if (event.key === 'ArrowUp') {
                                event.preventDefault();
                                setActiveIndex((prev) =>
                                    filtered.length > 0
                                        ? (prev - 1 + filtered.length) % filtered.length
                                        : 0,
                                );
                            } else if (event.key === 'Enter') {
                                event.preventDefault();
                                executeItem(filtered[safeIndex]);
                            }
                        }}
                        placeholder="搜索页面或快捷操作…"
                        className="w-full border-0 bg-transparent text-[14.5px] text-ui-text placeholder:text-[#889B98] focus:outline-none"
                    />
                    <kbd className="rounded-md border border-ui-line bg-ui-subtle px-1.5 py-0.5 font-code text-[11px] text-ui-sub">
                        ESC
                    </kbd>
                </div>

                <div className="max-h-[360px] overflow-y-auto p-2">
                    {filtered.length === 0 ? (
                        <div className="py-10 text-center text-[13.5px] text-ui-sub">
                            未找到匹配的页面或命令
                        </div>
                    ) : (
                        <ul className="space-y-0.5">
                            {filtered.map((item, idx) => {
                                const Icon = item.icon;
                                const isActive = idx === safeIndex;
                                const prevGroup = idx > 0 ? filtered[idx - 1]?.group : null;
                                const showGroup = item.group !== prevGroup;
                                return (
                                    <li key={item.id}>
                                        {showGroup ? (
                                            <div className="px-2.5 pt-2.5 pb-1 text-[11px] font-medium tracking-[0.04em] text-ui-sub">
                                                {item.group}
                                            </div>
                                        ) : null}
                                        <button
                                            type="button"
                                            onMouseEnter={() => setActiveIndex(idx)}
                                            onClick={() => executeItem(item)}
                                            className={cn(
                                                'flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors',
                                                isActive
                                                    ? 'bg-accent-tint text-accent-2'
                                                    : 'text-ui-text hover:bg-ui-subtle',
                                            )}
                                        >
                                            <span
                                                className={cn(
                                                    'grid h-7 w-7 shrink-0 place-items-center rounded-lg',
                                                    isActive
                                                        ? 'bg-accent text-white'
                                                        : 'bg-ui-bg text-ui-sub',
                                                )}
                                            >
                                                <Icon className="h-3.5 w-3.5" />
                                            </span>
                                            <span className="min-w-0 flex-1">
                                                <span className="block truncate text-[13.5px] font-medium">
                                                    {item.label}
                                                </span>
                                            </span>
                                            <span className="truncate font-code text-[11.5px] text-ui-sub">
                                                {item.hint}
                                            </span>
                                            {isActive ? (
                                                <CornerDownLeft className="h-3.5 w-3.5 shrink-0 text-accent" />
                                            ) : null}
                                        </button>
                                    </li>
                                );
                            })}
                        </ul>
                    )}
                </div>

                <div className="flex items-center justify-between border-t border-ui-line bg-ui-subtle px-4 py-2 text-[11.5px] text-ui-sub">
                    <span>↑ ↓ 移动 · Enter 打开</span>
                    <span className="font-code">Silva Studio</span>
                </div>
            </dialog>
        </div>
    );
}
