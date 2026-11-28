import { zodResolver } from '@hookform/resolvers/zod';
import type { Schemas } from '@silva/api-client';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Controller, useFieldArray, useForm, useWatch } from 'react-hook-form';
import { z } from 'zod';
import { AuthenticatedImage, SafeHtmlPreview } from '@/components/media/authenticated-media';
import { Button } from '@/components/ui/button';
import { DatePicker } from '@/components/ui/date-picker';
import { ErrorBanner, LoadingCard, PageContainer, PageHeader } from '@/components/ui/feedback';
import { useToast } from '@/components/ui/toast';
import { MediaPickerDrawer } from '@/features/media/media-picker-drawer';
import { apiClient, unwrapResponse } from '@/lib/api';
import { applyApiFieldErrors, formatErrorWithRequestId, isApiError } from '@/lib/errors';
import { renderSafeMarkdown } from '@/lib/markdown';
import { queryKeys } from '@/lib/query-keys';

const profileSchema = z.object({
    display_name: z.string().trim().min(1, '请输入显示名'),
    headline: z.string(),
    location: z.string(),
    bio_md: z.string(),
    avatar_media_id: z.string().nullable(),
    experiences: z.array(
        z.object({
            organization: z.string().trim().min(1, '请输入组织名称'),
            role: z.string().trim().min(1, '请输入职位'),
            started_on: z.string(),
            ended_on: z.string(),
            description_md: z.string(),
        }),
    ),
    skills: z.array(
        z.object({
            name: z.string().trim().min(1, '请输入技能名称'),
            category: z.string(),
            proficiency: z.string(),
        }),
    ),
    links: z.array(
        z.object({
            platform: z.string().trim().min(1, '请输入平台标识'),
            label: z.string(),
            url: z.string().trim().min(1, '请输入链接地址'),
        }),
    ),
});

type ProfileFormValues = z.infer<typeof profileSchema>;

const EMPTY_PROFILE: ProfileFormValues = {
    display_name: '',
    headline: '',
    location: '',
    bio_md: '',
    avatar_media_id: null,
    experiences: [],
    skills: [],
    links: [],
};

function publicProfileToFormValues(data: Schemas['PublicProfile']): ProfileFormValues {
    return {
        display_name: data.display_name,
        headline: data.headline ?? '',
        location: data.location ?? '',
        bio_md: '',
        avatar_media_id: data.avatar?.media_id ?? null,
        experiences: data.experiences.map((item) => ({
            organization: item.organization,
            role: item.role,
            started_on: item.started_on ?? '',
            ended_on: item.ended_on ?? '',
            description_md: '',
        })),
        skills: data.skills.map((item) => ({
            name: item.name,
            category: item.category ?? '',
            proficiency: item.proficiency ?? '',
        })),
        links: data.links.map((item) => ({
            platform: item.platform,
            label: item.label ?? '',
            url: item.url,
        })),
    };
}

function formValuesToApi(values: ProfileFormValues): Schemas['ProfileInput'] {
    return {
        display_name: values.display_name.trim(),
        headline: values.headline.trim() || null,
        location: values.location.trim() || null,
        bio_md: values.bio_md.trim() || null,
        avatar_media_id: values.avatar_media_id,
        experiences: values.experiences.map((exp) => ({
            organization: exp.organization.trim(),
            role: exp.role.trim(),
            started_on: exp.started_on.trim() || null,
            ended_on: exp.ended_on.trim() || null,
            description_md: exp.description_md.trim() || null,
        })),
        skills: values.skills.map((skill) => ({
            name: skill.name.trim(),
            category: skill.category.trim() || null,
            proficiency: skill.proficiency.trim() || null,
        })),
        links: values.links.map((link) => ({
            platform: link.platform.trim(),
            label: link.label.trim() || null,
            url: link.url.trim(),
        })),
    };
}

export function ProfilePage() {
    const queryClient = useQueryClient();
    const { toast } = useToast();

    const [bioTab, setBioTab] = useState<'src' | 'preview'>('src');
    const [avatarPickerOpen, setAvatarPickerOpen] = useState(false);
    const [topError, setTopError] = useState<string | null>(null);

    const profileQuery = useQuery({
        queryKey: queryKeys.profile.current,
        queryFn: async (): Promise<Schemas['PublicProfile'] | null> => {
            try {
                const res = await unwrapResponse(apiClient.GET('/api/v1/public/profile'));
                return res.data;
            } catch (error) {
                if (isApiError(error) && error.status === 404) {
                    return null;
                }
                throw error;
            }
        },
    });

    const {
        register,
        control,
        handleSubmit,
        reset,
        setValue,
        setError,
        formState: { errors, isDirty },
    } = useForm<ProfileFormValues>({
        resolver: zodResolver(profileSchema),
        defaultValues: EMPTY_PROFILE,
    });

    const expArray = useFieldArray({ control, name: 'experiences' });
    const skillArray = useFieldArray({ control, name: 'skills' });
    const linkArray = useFieldArray({ control, name: 'links' });

    useEffect(() => {
        if (profileQuery.isSuccess) {
            reset(profileQuery.data ? publicProfileToFormValues(profileQuery.data) : EMPTY_PROFILE);
        }
    }, [profileQuery.isSuccess, profileQuery.data, reset]);

    const saveMutation = useMutation({
        mutationFn: async (values: ProfileFormValues) => {
            const res = await unwrapResponse(
                apiClient.PUT('/api/v1/admin/users/profile', {
                    body: formValuesToApi(values),
                }),
            );
            return res.data;
        },
        onSuccess: (_, values) => {
            setTopError(null);
            reset(values);
            void queryClient.invalidateQueries({ queryKey: queryKeys.profile.current });
            void queryClient.invalidateQueries({ queryKey: queryKeys.publicSite.current });
            toast('个人资料已更新', 'ok');
        },
        onError: (error) => {
            const unmatched = applyApiFieldErrors(error, setError);
            if (unmatched.length > 0 || !isApiError(error) || error.code !== 'validation_error') {
                setTopError(formatErrorWithRequestId(error));
            }
        },
    });

    const avatarMediaId = useWatch({ control, name: 'avatar_media_id' });
    const displayName = useWatch({ control, name: 'display_name' });
    const bioMd = useWatch({ control, name: 'bio_md' });
    const watchedExperiences = useWatch({ control, name: 'experiences' });
    const bioPreviewHtml = useMemo(() => renderSafeMarkdown(bioMd), [bioMd]);

    if (profileQuery.isLoading) {
        return (
            <PageContainer size="narrow">
                <LoadingCard text="正在加载个人资料…" />
            </PageContainer>
        );
    }

    if (profileQuery.isError) {
        return (
            <PageContainer size="narrow">
                <ErrorBanner
                    title="个人资料加载失败"
                    error={profileQuery.error}
                    onRetry={() => void profileQuery.refetch()}
                />
            </PageContainer>
        );
    }

    const isFirstTime = profileQuery.data === null;

    return (
        <PageContainer size="narrow" className="space-y-6">
            <PageHeader
                eyebrow="PUBLIC PROFILE"
                title="个人资料"
                description="用于公开站点首页首屏、关于页与页脚联络。保存后同步更新公开资料。"
            />

            {isFirstTime ? (
                <div className="rounded-xl border border-[#BBDCD8] bg-accent-tint px-4 py-3 text-[13.5px] text-accent-2">
                    当前尚未配置个人资料。填写下方信息并保存后，访客即可查看公开资料。
                </div>
            ) : null}

            {topError ? (
                <div
                    role="alert"
                    className="rounded-xl border border-[#F0C2BD] bg-bad-tint px-4 py-3 text-[13.5px] text-bad"
                >
                    {topError}
                </div>
            ) : null}

            <form
                className="space-y-6"
                onSubmit={handleSubmit((values) => {
                    setTopError(null);
                    saveMutation.mutate(values);
                })}
                noValidate
            >
                {/* 身份与头像 */}
                <section aria-labelledby="profile-identity" className="studio-card p-6">
                    <h2 id="profile-identity" className="text-[16px] font-semibold text-ui-text">
                        身份与头像
                    </h2>
                    <div className="mt-4 grid gap-6 sm:grid-cols-[120px_1fr]">
                        <div>
                            <div className="h-[96px] w-[96px] overflow-hidden rounded-full border-2 border-ui-line bg-rail text-white shadow-xs">
                                {avatarMediaId ? (
                                    <AuthenticatedImage
                                        mediaId={avatarMediaId}
                                        alt="个人头像"
                                        className="h-full w-full object-cover"
                                    />
                                ) : (
                                    <div className="grid h-full w-full place-items-center text-[32px] font-semibold tracking-tight text-[#5ED6CC]">
                                        {displayName.trim().slice(0, 1) || 'S'}
                                    </div>
                                )}
                            </div>
                            <div className="mt-2.5 flex flex-col items-start gap-1 text-[13px]">
                                <button
                                    type="button"
                                    onClick={() => setAvatarPickerOpen(true)}
                                    className="font-medium text-accent hover:underline"
                                >
                                    从媒体库选择
                                </button>
                                {avatarMediaId ? (
                                    <button
                                        type="button"
                                        onClick={() =>
                                            setValue('avatar_media_id', null, { shouldDirty: true })
                                        }
                                        className="text-ui-sub hover:text-bad"
                                    >
                                        移除头像
                                    </button>
                                ) : null}
                            </div>
                            <p className="mt-1 text-[12px] text-ui-sub">无头像时显示姓名首字。</p>
                        </div>

                        <div className="grid gap-4 sm:grid-cols-2">
                            <div>
                                <label
                                    htmlFor="profile-display-name"
                                    className="block text-[13px] font-medium"
                                >
                                    显示名
                                </label>
                                <input
                                    id="profile-display-name"
                                    type="text"
                                    className="studio-input mt-1.5"
                                    {...register('display_name')}
                                />
                                {errors.display_name ? (
                                    <p className="mt-1 text-[12px] text-bad">
                                        {errors.display_name.message}
                                    </p>
                                ) : null}
                            </div>

                            <div>
                                <label
                                    htmlFor="profile-location"
                                    className="block text-[13px] font-medium"
                                >
                                    所在地 <span className="font-normal text-ui-sub">（可选）</span>
                                </label>
                                <input
                                    id="profile-location"
                                    type="text"
                                    placeholder="例如：四川，成都"
                                    className="studio-input mt-1.5"
                                    {...register('location')}
                                />
                            </div>

                            <div className="sm:col-span-2">
                                <label
                                    htmlFor="profile-headline"
                                    className="block text-[13px] font-medium"
                                >
                                    一句话定位 / 头衔{' '}
                                    <span className="font-normal text-ui-sub">（可选）</span>
                                </label>
                                <input
                                    id="profile-headline"
                                    type="text"
                                    placeholder="例如：全栈开发工程师 · 把复杂的技术写成清晰的路径"
                                    className="studio-input mt-1.5"
                                    {...register('headline')}
                                />
                            </div>
                        </div>
                    </div>
                </section>

                {/* Markdown 简介 */}
                <section aria-labelledby="profile-bio" className="studio-card p-6">
                    <div className="flex items-center justify-between">
                        <h2 id="profile-bio" className="text-[16px] font-semibold text-ui-text">
                            个人简介
                        </h2>
                        <div
                            role="tablist"
                            aria-label="简介编辑模式"
                            className="inline-flex rounded-xl border border-ui-line bg-ui-subtle p-1 text-[13px]"
                        >
                            <button
                                type="button"
                                role="tab"
                                aria-selected={bioTab === 'src'}
                                onClick={() => setBioTab('src')}
                                className="rounded-lg px-3 py-1 font-medium text-ui-sub transition-colors aria-selected:bg-white aria-selected:text-ui-text aria-selected:shadow-2xs"
                            >
                                Markdown
                            </button>
                            <button
                                type="button"
                                role="tab"
                                aria-selected={bioTab === 'preview'}
                                onClick={() => setBioTab('preview')}
                                className="rounded-lg px-3 py-1 font-medium text-ui-sub transition-colors aria-selected:bg-white aria-selected:text-ui-text aria-selected:shadow-2xs"
                            >
                                预览
                            </button>
                        </div>
                    </div>
                    <div className="mt-3">
                        {bioTab === 'src' ? (
                            <textarea
                                rows={8}
                                aria-labelledby="profile-bio"
                                placeholder="用 Markdown 介绍你的背景、关注方向与自叙……"
                                className="studio-input h-auto py-2.5 font-code text-[13.5px] leading-6"
                                {...register('bio_md')}
                            />
                        ) : (
                            <div className="min-h-[180px] rounded-xl border border-ui-line bg-paper p-5">
                                <SafeHtmlPreview html={bioPreviewHtml} emptyText="尚未填写简介。" />
                            </div>
                        )}
                        <p className="mt-1.5 text-[12px] text-ui-sub">
                            预览经过安全消毒，原始 HTML 不会输出。
                        </p>
                    </div>
                </section>

                {/* 经历 */}
                <section aria-labelledby="profile-exp" className="studio-card p-6">
                    <div className="flex items-center justify-between">
                        <h2 id="profile-exp" className="text-[16px] font-semibold text-ui-text">
                            经历
                        </h2>
                        <Button
                            variant="ghost"
                            size="sm"
                            className="text-accent"
                            onClick={() =>
                                expArray.append({
                                    organization: '',
                                    role: '',
                                    started_on: '',
                                    ended_on: '',
                                    description_md: '',
                                })
                            }
                        >
                            <Plus className="h-4 w-4" />
                            添加经历
                        </Button>
                    </div>
                    <p className="mt-1 text-[12px] text-ui-sub">
                        按列表顺序展示。日期格式为
                        YYYY-MM-DD，可留空；未填结束日期不会自动推断为「至今」。
                    </p>

                    {expArray.fields.length === 0 ? (
                        <p className="mt-4 rounded-md border border-dashed border-ui-line py-6 text-center text-[13px] text-ui-sub">
                            暂无经历条目，点击右上角添加。
                        </p>
                    ) : (
                        <ol className="mt-4 space-y-3">
                            {expArray.fields.map((field, index) => (
                                <li key={field.id} className="rounded-md border border-ui-line p-4">
                                    <div className="flex items-center justify-between">
                                        <span className="tnum text-[13px] text-ui-sub">
                                            第 {index + 1} 条
                                        </span>
                                        <div className="flex items-center gap-1">
                                            <button
                                                type="button"
                                                aria-label="上移"
                                                disabled={index === 0}
                                                onClick={() => expArray.swap(index, index - 1)}
                                                className="rounded p-1 text-ui-sub hover:bg-ui-bg disabled:opacity-30"
                                            >
                                                <ArrowUp className="h-4 w-4" />
                                            </button>
                                            <button
                                                type="button"
                                                aria-label="下移"
                                                disabled={index === expArray.fields.length - 1}
                                                onClick={() => expArray.swap(index, index + 1)}
                                                className="rounded p-1 text-ui-sub hover:bg-ui-bg disabled:opacity-30"
                                            >
                                                <ArrowDown className="h-4 w-4" />
                                            </button>
                                            <button
                                                type="button"
                                                aria-label="移除经历"
                                                onClick={() => expArray.remove(index)}
                                                className="rounded p-1 text-bad hover:bg-bad-tint"
                                            >
                                                <Trash2 className="h-4 w-4" />
                                            </button>
                                        </div>
                                    </div>

                                    <div className="mt-2 grid gap-3 sm:grid-cols-2">
                                        <div>
                                            <label className="text-[12px] text-ui-sub">
                                                组织
                                                <input
                                                    type="text"
                                                    className="mt-1 h-9 w-full rounded-md border border-ui-line px-3 text-[14px] text-ui-text focus:border-accent focus:outline-none"
                                                    {...register(
                                                        `experiences.${index}.organization`,
                                                    )}
                                                />
                                            </label>
                                            {errors.experiences?.[index]?.organization ? (
                                                <p className="mt-1 text-[12px] text-bad">
                                                    {errors.experiences[index].organization.message}
                                                </p>
                                            ) : null}
                                        </div>
                                        <div>
                                            <label className="text-[12px] text-ui-sub">
                                                职位
                                                <input
                                                    type="text"
                                                    className="mt-1 h-9 w-full rounded-md border border-ui-line px-3 text-[14px] text-ui-text focus:border-accent focus:outline-none"
                                                    {...register(`experiences.${index}.role`)}
                                                />
                                            </label>
                                            {errors.experiences?.[index]?.role ? (
                                                <p className="mt-1 text-[12px] text-bad">
                                                    {errors.experiences[index].role.message}
                                                </p>
                                            ) : null}
                                        </div>
                                        <div>
                                            <label
                                                htmlFor={`exp-${field.id}-started-on`}
                                                className="block text-[12px] text-ui-sub"
                                            >
                                                开始日期（可选）
                                            </label>
                                            <Controller
                                                control={control}
                                                name={`experiences.${index}.started_on`}
                                                render={({ field: dateField }) => (
                                                    <DatePicker
                                                        id={`exp-${field.id}-started-on`}
                                                        value={dateField.value}
                                                        onChange={dateField.onChange}
                                                        placeholder="选择开始日期"
                                                        ariaLabel={`第 ${index + 1} 条经历开始日期`}
                                                        className="mt-1"
                                                    />
                                                )}
                                            />
                                            {errors.experiences?.[index]?.started_on ? (
                                                <p className="mt-1 text-[12px] text-bad">
                                                    {errors.experiences[index].started_on.message}
                                                </p>
                                            ) : null}
                                        </div>
                                        <div>
                                            <label
                                                htmlFor={`exp-${field.id}-ended-on`}
                                                className="block text-[12px] text-ui-sub"
                                            >
                                                结束日期（可选）
                                            </label>
                                            <Controller
                                                control={control}
                                                name={`experiences.${index}.ended_on`}
                                                render={({ field: dateField }) => (
                                                    <DatePicker
                                                        id={`exp-${field.id}-ended-on`}
                                                        value={dateField.value}
                                                        onChange={dateField.onChange}
                                                        placeholder="选择结束日期"
                                                        ariaLabel={`第 ${index + 1} 条经历结束日期`}
                                                        min={
                                                            watchedExperiences?.[index]
                                                                ?.started_on || undefined
                                                        }
                                                        className="mt-1"
                                                    />
                                                )}
                                            />
                                            {errors.experiences?.[index]?.ended_on ? (
                                                <p className="mt-1 text-[12px] text-bad">
                                                    {errors.experiences[index].ended_on.message}
                                                </p>
                                            ) : null}
                                        </div>
                                        <div className="sm:col-span-2">
                                            <label className="text-[12px] text-ui-sub">
                                                说明（支持 Markdown，可选）
                                                <textarea
                                                    rows={2}
                                                    className="mt-1 w-full rounded-md border border-ui-line px-3 py-2 text-[14px] text-ui-text focus:border-accent focus:outline-none"
                                                    {...register(
                                                        `experiences.${index}.description_md`,
                                                    )}
                                                />
                                            </label>
                                        </div>
                                    </div>
                                </li>
                            ))}
                        </ol>
                    )}
                </section>

                {/* 技能 */}
                <section aria-labelledby="profile-skills" className="studio-card p-6">
                    <div className="flex items-center justify-between">
                        <h2 id="profile-skills" className="text-[16px] font-semibold text-ui-text">
                            技能
                        </h2>
                        <Button
                            variant="ghost"
                            size="sm"
                            className="text-accent"
                            onClick={() =>
                                skillArray.append({ name: '', category: '', proficiency: '' })
                            }
                        >
                            <Plus className="h-4 w-4" />
                            添加技能
                        </Button>
                    </div>
                    <p className="mt-1 text-[12px] text-ui-sub">
                        按分类分组展示；未填分类归入「其他」。熟练程度按原文显示，不换算百分比。
                    </p>

                    {skillArray.fields.length === 0 ? (
                        <p className="mt-4 rounded-md border border-dashed border-ui-line py-6 text-center text-[13px] text-ui-sub">
                            暂无技能条目，点击右上角添加。
                        </p>
                    ) : (
                        <div className="mt-4 overflow-x-auto">
                            <table className="tbl min-w-[560px]">
                                <thead>
                                    <tr>
                                        <th className="w-10">顺序</th>
                                        <th>技能</th>
                                        <th>分类</th>
                                        <th>熟练程度（可选）</th>
                                        <th className="text-right">操作</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {skillArray.fields.map((field, index) => (
                                        <tr key={field.id}>
                                            <td className="tnum text-ui-sub">{index + 1}</td>
                                            <td>
                                                <input
                                                    aria-label="技能名称"
                                                    type="text"
                                                    className="h-9 w-full rounded-md border border-ui-line px-3 text-[14px]"
                                                    {...register(`skills.${index}.name`)}
                                                />
                                                {errors.skills?.[index]?.name ? (
                                                    <p className="mt-1 text-[12px] text-bad">
                                                        {errors.skills[index].name.message}
                                                    </p>
                                                ) : null}
                                            </td>
                                            <td>
                                                <input
                                                    aria-label="分类"
                                                    type="text"
                                                    placeholder="其他"
                                                    className="h-9 w-full rounded-md border border-ui-line px-3 text-[14px]"
                                                    {...register(`skills.${index}.category`)}
                                                />
                                            </td>
                                            <td>
                                                <input
                                                    aria-label="熟练程度"
                                                    type="text"
                                                    placeholder="不填则不显示"
                                                    className="h-9 w-full rounded-md border border-ui-line px-3 text-[14px]"
                                                    {...register(`skills.${index}.proficiency`)}
                                                />
                                            </td>
                                            <td className="whitespace-nowrap text-right">
                                                <button
                                                    type="button"
                                                    aria-label="上移"
                                                    disabled={index === 0}
                                                    onClick={() =>
                                                        skillArray.swap(index, index - 1)
                                                    }
                                                    className="rounded p-1 text-ui-sub hover:bg-ui-bg disabled:opacity-30"
                                                >
                                                    <ArrowUp className="h-4 w-4" />
                                                </button>
                                                <button
                                                    type="button"
                                                    aria-label="下移"
                                                    disabled={
                                                        index === skillArray.fields.length - 1
                                                    }
                                                    onClick={() =>
                                                        skillArray.swap(index, index + 1)
                                                    }
                                                    className="rounded p-1 text-ui-sub hover:bg-ui-bg disabled:opacity-30"
                                                >
                                                    <ArrowDown className="h-4 w-4" />
                                                </button>
                                                <button
                                                    type="button"
                                                    aria-label="移除技能"
                                                    onClick={() => skillArray.remove(index)}
                                                    className="rounded p-1 text-bad hover:bg-bad-tint"
                                                >
                                                    <Trash2 className="h-4 w-4" />
                                                </button>
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </div>
                    )}
                </section>

                {/* 社交链接 */}
                <section aria-labelledby="profile-links" className="studio-card p-6">
                    <div className="flex items-center justify-between">
                        <h2 id="profile-links" className="text-[16px] font-semibold text-ui-text">
                            公开社交链接
                        </h2>
                        <Button
                            variant="ghost"
                            size="sm"
                            className="text-accent"
                            onClick={() => linkArray.append({ platform: '', label: '', url: '' })}
                        >
                            <Plus className="h-4 w-4" />
                            添加链接
                        </Button>
                    </div>
                    <p className="mt-1 text-[12px] text-ui-sub">
                        只有填写的链接会公开显示。支持 https:// 或 mailto: 地址。
                    </p>

                    {linkArray.fields.length === 0 ? (
                        <p className="mt-4 rounded-md border border-dashed border-ui-line py-6 text-center text-[13px] text-ui-sub">
                            暂无社交链接，点击右上角添加。
                        </p>
                    ) : (
                        <ol className="mt-4 space-y-2.5">
                            {linkArray.fields.map((field, index) => (
                                <li
                                    key={field.id}
                                    className="grid items-start gap-2 sm:grid-cols-[120px_140px_1fr_auto]"
                                >
                                    <div>
                                        <input
                                            aria-label="平台标识"
                                            type="text"
                                            placeholder="如 github"
                                            className="h-9 w-full rounded-md border border-ui-line px-3 text-[14px]"
                                            {...register(`links.${index}.platform`)}
                                        />
                                        {errors.links?.[index]?.platform ? (
                                            <p className="mt-1 text-[12px] text-bad">
                                                {errors.links[index].platform.message}
                                            </p>
                                        ) : null}
                                    </div>
                                    <input
                                        aria-label="显示文案"
                                        type="text"
                                        placeholder="显示名（可选）"
                                        className="h-9 w-full rounded-md border border-ui-line px-3 text-[14px]"
                                        {...register(`links.${index}.label`)}
                                    />
                                    <div>
                                        <input
                                            aria-label="链接地址"
                                            type="text"
                                            placeholder="https://github.com/..."
                                            className="h-9 w-full rounded-md border border-ui-line px-3 font-code text-[13px]"
                                            {...register(`links.${index}.url`)}
                                        />
                                        {errors.links?.[index]?.url ? (
                                            <p className="mt-1 text-[12px] text-bad">
                                                {errors.links[index].url.message}
                                            </p>
                                        ) : null}
                                    </div>
                                    <div className="flex items-center gap-1 pt-0.5">
                                        <button
                                            type="button"
                                            aria-label="上移"
                                            disabled={index === 0}
                                            onClick={() => linkArray.swap(index, index - 1)}
                                            className="rounded p-1.5 text-ui-sub hover:bg-ui-bg disabled:opacity-30"
                                        >
                                            <ArrowUp className="h-4 w-4" />
                                        </button>
                                        <button
                                            type="button"
                                            aria-label="下移"
                                            disabled={index === linkArray.fields.length - 1}
                                            onClick={() => linkArray.swap(index, index + 1)}
                                            className="rounded p-1.5 text-ui-sub hover:bg-ui-bg disabled:opacity-30"
                                        >
                                            <ArrowDown className="h-4 w-4" />
                                        </button>
                                        <button
                                            type="button"
                                            aria-label="移除链接"
                                            onClick={() => linkArray.remove(index)}
                                            className="rounded p-1.5 text-bad hover:bg-bad-tint"
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </button>
                                    </div>
                                </li>
                            ))}
                        </ol>
                    )}
                </section>

                {/* 吸底统一保存条 */}
                <div className="sticky bottom-3 -mx-4 flex flex-wrap items-center justify-between gap-3 border-t border-ui-line bg-white/95 px-4 py-3 shadow-[0_-4px_20px_rgba(9,28,26,0.06)] backdrop-blur md:mx-0 md:rounded-2xl md:border md:px-5">
                    <span className="text-[13px] text-ui-sub" aria-live="polite">
                        {isDirty ? '有未保存的修改' : '当前内容已同步'}
                    </span>
                    <div className="flex gap-2">
                        <Button
                            variant="secondary"
                            disabled={!isDirty || saveMutation.isPending}
                            onClick={() =>
                                reset(
                                    profileQuery.data
                                        ? publicProfileToFormValues(profileQuery.data)
                                        : EMPTY_PROFILE,
                                )
                            }
                        >
                            放弃修改
                        </Button>
                        <Button variant="primary" type="submit" disabled={saveMutation.isPending}>
                            {saveMutation.isPending ? '保存中…' : '保存资料'}
                        </Button>
                    </div>
                </div>
            </form>

            <MediaPickerDrawer
                open={avatarPickerOpen}
                onClose={() => setAvatarPickerOpen(false)}
                title="选择头像媒体"
                confirmLabel="设为头像"
                onSelect={(media) => setValue('avatar_media_id', media.id, { shouldDirty: true })}
            />
        </PageContainer>
    );
}
