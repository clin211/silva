import { zodResolver } from '@hookform/resolvers/zod';
import { useNavigate, useSearch } from '@tanstack/react-router';
import { Eye, EyeOff, GitBranch, Lock, Mail, Rocket, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';
import { useAuth } from './auth-context';
import { SilvaBrandMark } from '@/components/layout/admin-shell';
import { Button } from '@/components/ui/button';
import { applyApiFieldErrors, formatErrorWithRequestId, isApiError } from '@/lib/errors';

const loginSchema = z.object({
    email: z.string().trim().email('请输入有效的邮箱地址'),
    password: z.string().min(1, '请输入密码'),
});

type LoginFormValues = z.infer<typeof loginSchema>;

export function LoginPage() {
    const navigate = useNavigate();
    const search = useSearch({ from: '/login' });
    const { status, login, sessionExpired, clearExpiredFlag } = useAuth();
    const [showPassword, setShowPassword] = useState(false);
    const [submitError, setSubmitError] = useState<{
        tone: 'bad' | 'warn';
        title: string;
        detail: string;
    } | null>(null);

    const {
        register,
        handleSubmit,
        setError,
        formState: { errors, isSubmitting },
    } = useForm<LoginFormValues>({
        resolver: zodResolver(loginSchema),
        defaultValues: { email: '', password: '' },
    });

    const redirectTarget =
        search.redirect && search.redirect.startsWith('/') && !search.redirect.startsWith('//')
            ? search.redirect
            : '/';

    useEffect(() => {
        if (status === 'authenticated') {
            void navigate({ to: redirectTarget as '/' });
        }
    }, [status, navigate, redirectTarget]);

    const showExpiredNotice = Boolean(search.expired || sessionExpired);

    const onSubmit = async (values: LoginFormValues) => {
        setSubmitError(null);
        clearExpiredFlag();
        try {
            await login(values);
            await navigate({ to: redirectTarget as '/' });
        } catch (error) {
            if (isApiError(error)) {
                if (error.code === 'validation_error') {
                    applyApiFieldErrors(error, setError);
                    return;
                }
                if (error.code === 'rate_limited') {
                    setSubmitError({
                        tone: 'warn',
                        title: '尝试次数过多',
                        detail: error.message,
                    });
                    return;
                }
                setSubmitError({
                    tone: 'bad',
                    title: '登录失败',
                    detail: formatErrorWithRequestId(error),
                });
                return;
            }
            setSubmitError({
                tone: 'bad',
                title: '登录失败',
                detail: formatErrorWithRequestId(error),
            });
        }
    };

    return (
        <div className="min-h-screen bg-ui-bg lg:grid lg:grid-cols-[minmax(0,1.08fr)_minmax(0,1fr)]">
            {/* 左侧：Silva Studio 品牌叙事面板（清新翡翠绿渐变） */}
            <aside className="hero-emerald-card silva-dot-grid relative hidden flex-col justify-between overflow-hidden p-12 text-white lg:flex xl:p-16">
                <div
                    aria-hidden="true"
                    className="pointer-events-none absolute -top-28 -right-28 h-[420px] w-[420px] rounded-full bg-white/10 blur-3xl"
                />
                <div
                    aria-hidden="true"
                    className="pointer-events-none absolute -bottom-32 -left-24 h-[360px] w-[360px] rounded-full bg-[#34D399]/18 blur-3xl"
                />

                <div className="relative z-10 flex items-center gap-3">
                    <SilvaBrandMark className="h-9 w-9 ring-1 ring-white/25" />
                    <span className="flex items-center gap-2">
                        <span className="text-[19px] font-semibold tracking-[-0.015em]">Silva</span>
                        <span className="rounded-md bg-white/15 px-2 py-0.5 font-code text-[11px] font-medium tracking-[0.08em] text-white/90">
                            STUDIO
                        </span>
                    </span>
                </div>

                <div className="relative z-10 max-w-[480px]">
                    <p className="font-code text-[12px] tracking-[0.08em] text-[#A7F3D0] uppercase">
                        Headless CMS · Publishing Console
                    </p>
                    <h2 className="mt-3 text-[34px] leading-[1.2] font-semibold tracking-[-0.025em] text-white">
                        内容管理工作台
                    </h2>
                    <p className="mt-4 text-[15px] leading-7 text-white/80">
                        保存草稿，并在内容准备好时发布到站点。
                    </p>

                    <ul className="mt-8 space-y-3.5">
                        <li className="flex items-start gap-3.5 rounded-xl border border-white/15 bg-white/10 p-3.5 backdrop-blur-xs">
                            <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-white/18 text-white">
                                <GitBranch className="h-4 w-4" />
                            </span>
                            <div>
                                <div className="text-[14px] font-medium text-white">
                                    自动保存草稿
                                </div>
                                <div className="mt-0.5 text-[12.5px] text-white/75">
                                    编辑时自动保存草稿，发布后站点会展示最新内容。
                                </div>
                            </div>
                        </li>
                        <li className="flex items-start gap-3.5 rounded-xl border border-white/15 bg-white/10 p-3.5 backdrop-blur-xs">
                            <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-white/18 text-white">
                                <Rocket className="h-4 w-4" />
                            </span>
                            <div>
                                <div className="text-[14px] font-medium text-white">
                                    一键发布文章
                                </div>
                                <div className="mt-0.5 text-[12.5px] text-white/75">
                                    点击发布即可更新正文、文章列表和订阅。
                                </div>
                            </div>
                        </li>
                        <li className="flex items-start gap-3.5 rounded-xl border border-white/15 bg-white/10 p-3.5 backdrop-blur-xs">
                            <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-white/18 text-white">
                                <ShieldCheck className="h-4 w-4" />
                            </span>
                            <div>
                                <div className="text-[14px] font-medium text-white">
                                    RBAC 与全链路审计
                                </div>
                                <div className="mt-0.5 text-[12.5px] text-white/75">
                                    内置 Author / Editor / Admin 权限边界与完整写操作审计追踪。
                                </div>
                            </div>
                        </li>
                    </ul>
                </div>

                <div className="relative z-10 flex items-center justify-between text-[12px] text-white/70">
                    <span>Silva Developer Hub</span>
                </div>
            </aside>

            {/* 右侧：登录表单工作区 */}
            <div className="grid min-h-screen place-items-center px-5 py-10">
                <main id="main" className="w-full max-w-[420px]">
                    <div className="mb-8 flex items-center justify-center gap-2.5 lg:hidden">
                        <SilvaBrandMark className="h-9 w-9" />
                        <span className="text-[19px] font-semibold tracking-[-0.015em] text-ui-text">
                            Silva Studio
                        </span>
                    </div>

                    <div className="studio-card p-7 sm:p-9">
                        <h1 className="text-[22px] font-semibold tracking-[-0.02em] text-ui-text">
                            登录管理后台
                        </h1>
                        <p className="mt-1.5 text-[13.5px] text-ui-sub">
                            使用管理员分配的邮箱与密码登录工作台。
                        </p>

                        {showExpiredNotice && !submitError ? (
                            <output className="mt-5 block rounded-xl border border-accent/25 bg-accent-tint px-3.5 py-3 text-[13.5px] text-accent-2">
                                会话已失效，请重新登录。登录后会回到刚才的页面。
                            </output>
                        ) : null}

                        {submitError ? (
                            <div
                                role="alert"
                                className={
                                    submitError.tone === 'warn'
                                        ? 'mt-5 rounded-xl border border-warn/25 bg-warn-tint px-3.5 py-3 text-[13.5px] text-warn'
                                        : 'mt-5 rounded-xl border border-bad/25 bg-bad-tint px-3.5 py-3 text-[13.5px] text-bad'
                                }
                            >
                                <strong className="font-medium">{submitError.title}：</strong>
                                {submitError.detail}
                            </div>
                        ) : null}

                        <form
                            className="mt-6 space-y-4"
                            onSubmit={handleSubmit(onSubmit)}
                            noValidate
                        >
                            <div>
                                <label
                                    htmlFor="login-email"
                                    className="block text-[13px] font-medium text-ui-text"
                                >
                                    邮箱
                                </label>
                                <div className="relative mt-1.5">
                                    <Mail
                                        aria-hidden="true"
                                        className="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[#889B98]"
                                    />
                                    <input
                                        id="login-email"
                                        type="email"
                                        autoComplete="username"
                                        aria-invalid={Boolean(errors.email)}
                                        placeholder="admin@example.com"
                                        className="studio-input h-10.5 w-full pl-10"
                                        {...register('email')}
                                    />
                                </div>
                                {errors.email ? (
                                    <p className="mt-1.5 text-[12.5px] text-bad">
                                        {errors.email.message}
                                    </p>
                                ) : null}
                            </div>

                            <div>
                                <label
                                    htmlFor="login-password"
                                    className="block text-[13px] font-medium text-ui-text"
                                >
                                    密码
                                </label>
                                <div className="relative mt-1.5">
                                    <Lock
                                        aria-hidden="true"
                                        className="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[#889B98]"
                                    />
                                    <input
                                        id="login-password"
                                        type={showPassword ? 'text' : 'password'}
                                        autoComplete="current-password"
                                        aria-invalid={Boolean(errors.password)}
                                        placeholder="输入登录密码"
                                        className="studio-input h-10.5 w-full pr-10 pl-10"
                                        {...register('password')}
                                    />
                                    <button
                                        type="button"
                                        onClick={() => setShowPassword((prev) => !prev)}
                                        aria-controls="login-password"
                                        aria-pressed={showPassword}
                                        aria-label={showPassword ? '隐藏密码' : '显示密码'}
                                        title={showPassword ? '隐藏密码' : '显示密码'}
                                        className="absolute top-1/2 right-1.5 grid h-8 w-8 -translate-y-1/2 place-items-center rounded-lg text-[#7D8C86] transition-colors hover:bg-[#F3F6F5] hover:text-ui-text"
                                    >
                                        {showPassword ? (
                                            <EyeOff className="h-4 w-4" aria-hidden="true" />
                                        ) : (
                                            <Eye className="h-4 w-4" aria-hidden="true" />
                                        )}
                                    </button>
                                </div>
                                {errors.password ? (
                                    <p className="mt-1.5 text-[12.5px] text-bad">
                                        {errors.password.message}
                                    </p>
                                ) : null}
                            </div>

                            <Button
                                type="submit"
                                variant="primary"
                                size="lg"
                                disabled={isSubmitting}
                                className="mt-2 h-11 w-full"
                            >
                                {isSubmitting ? '正在登录…' : '登录'}
                            </Button>
                        </form>
                    </div>

                    <p className="mt-6 text-center text-[12.5px] text-ui-sub">
                        本系统不提供公开注册，如需账号请联系站点管理员开通。
                    </p>
                </main>
            </div>
        </div>
    );
}
