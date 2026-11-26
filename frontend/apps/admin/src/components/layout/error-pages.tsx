import { Link, useLocation } from '@tanstack/react-router';
import { ArrowLeft, Compass, ShieldAlert } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useAuth } from '@/features/auth/auth-context';
import { getPrimaryRoleLabel } from '@/lib/permissions';

export function ForbiddenPage() {
    const { user } = useAuth();
    const location = useLocation();
    const roleLabel = getPrimaryRoleLabel(user?.roles);

    return (
        <div className="mx-auto max-w-lg py-16 md:py-24">
            <div className="studio-card px-8 py-12 text-center">
                <div className="mx-auto grid h-14 w-14 place-items-center rounded-2xl bg-bad-tint text-bad">
                    <ShieldAlert className="h-6 w-6" aria-hidden="true" />
                </div>
                <p className="tnum mt-5 font-code text-[13px] font-medium tracking-[0.08em] text-bad">
                    HTTP 403
                </p>
                <h1 className="mt-2 text-[22px] font-semibold tracking-[-0.02em] text-ui-text">
                    无权访问
                </h1>
                <p className="mt-2.5 text-[14px] leading-7 text-ui-sub">
                    当前账户（{roleLabel}）没有权限打开该页面。如需访问，请联系 Admin 调整角色。
                </p>
                <p className="mt-3 inline-block rounded-lg bg-ui-bg px-3 py-1 font-code text-[12px] text-ui-sub">
                    {location.pathname}
                </p>
                <div className="mt-7 flex flex-wrap justify-center gap-2.5">
                    <Link
                        to="/"
                        className="inline-flex h-10 items-center rounded-[10px] bg-accent px-5 text-[14px] font-medium text-white shadow-xs transition-colors hover:bg-accent-2"
                    >
                        回工作台
                    </Link>
                    <Button variant="secondary" size="lg" onClick={() => window.history.back()}>
                        <ArrowLeft className="h-4 w-4" />
                        返回上一页
                    </Button>
                </div>
            </div>
        </div>
    );
}

export function NotFoundPage() {
    const location = useLocation();

    return (
        <div className="mx-auto max-w-lg py-16 md:py-24">
            <div className="studio-card px-8 py-12 text-center">
                <div className="mx-auto grid h-14 w-14 place-items-center rounded-2xl bg-accent-tint text-accent">
                    <Compass className="h-6 w-6" aria-hidden="true" />
                </div>
                <p className="tnum mt-5 font-code text-[13px] font-medium tracking-[0.08em] text-accent">
                    HTTP 404
                </p>
                <h1 className="mt-2 text-[22px] font-semibold tracking-[-0.02em] text-ui-text">
                    页面不存在
                </h1>
                <p className="mt-2.5 text-[14px] leading-7 text-ui-sub">
                    这个地址没有对应的后台页面，或者访问的记录已被删除。请检查链接，或从导航重新进入。
                </p>
                <p className="mt-3 inline-block rounded-lg bg-ui-bg px-3 py-1 font-code text-[12px] text-ui-sub">
                    请求路径 {location.pathname}
                </p>
                <div className="mt-7 flex flex-wrap justify-center gap-2.5">
                    <Link
                        to="/"
                        className="inline-flex h-10 items-center rounded-[10px] bg-accent px-5 text-[14px] font-medium text-white shadow-xs transition-colors hover:bg-accent-2"
                    >
                        回工作台
                    </Link>
                    <Button variant="secondary" size="lg" onClick={() => window.history.back()}>
                        <ArrowLeft className="h-4 w-4" />
                        返回上一页
                    </Button>
                </div>
            </div>
        </div>
    );
}
