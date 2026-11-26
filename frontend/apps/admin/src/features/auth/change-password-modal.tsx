import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';
import { Button } from '@/components/ui/button';
import { Modal } from '@/components/ui/overlay';
import { useToast } from '@/components/ui/toast';
import { apiClient, unwrapResponse } from '@/lib/api';
import { applyApiFieldErrors, formatErrorWithRequestId } from '@/lib/errors';
import { utf8ByteLength } from '@/lib/format';

const changePasswordSchema = z
    .object({
        current_password: z.string().min(1, '请输入当前密码'),
        new_password: z
            .string()
            .refine((value) => utf8ByteLength(value) >= 12, '新密码至少需要 12 个字节')
            .refine((value) => utf8ByteLength(value) <= 1024, '新密码不能超过 1024 字节'),
        confirm_password: z.string().min(1, '请再次输入新密码'),
    })
    .refine((data) => data.new_password === data.confirm_password, {
        path: ['confirm_password'],
        message: '两次输入的新密码不一致',
    })
    .refine((data) => data.new_password !== data.current_password, {
        path: ['new_password'],
        message: '新密码不能与当前密码相同',
    });

type ChangePasswordValues = z.infer<typeof changePasswordSchema>;

export function ChangePasswordModal({ open, onClose }: { open: boolean; onClose: () => void }) {
    const { toast } = useToast();
    const [topError, setTopError] = useState<string | null>(null);

    const {
        register,
        handleSubmit,
        reset,
        setError,
        formState: { errors },
    } = useForm<ChangePasswordValues>({
        resolver: zodResolver(changePasswordSchema),
        defaultValues: {
            current_password: '',
            new_password: '',
            confirm_password: '',
        },
    });

    const handleClose = () => {
        reset();
        setTopError(null);
        onClose();
    };

    const mutation = useMutation({
        mutationFn: async (values: ChangePasswordValues) => {
            await unwrapResponse(
                apiClient.PUT('/api/v1/admin/users/me/password', {
                    body: {
                        current_password: values.current_password,
                        new_password: values.new_password,
                    },
                }),
            );
        },
        onSuccess: () => {
            toast('密码已更新，其他登录会话已自动失效。', 'ok');
            handleClose();
        },
        onError: (error) => {
            const unmatched = applyApiFieldErrors(error, setError);
            if (unmatched.length > 0 || Object.keys(errors).length === 0) {
                setTopError(formatErrorWithRequestId(error));
            }
        },
    });

    return (
        <Modal
            open={open}
            onClose={handleClose}
            title="修改密码"
            description="修改密码后当前会话保持有效，你在其他设备上的登录会话会立即失效。"
            footer={
                <>
                    <Button variant="secondary" onClick={handleClose}>
                        取消
                    </Button>
                    <Button
                        variant="primary"
                        type="submit"
                        form="change-password-form"
                        disabled={mutation.isPending}
                    >
                        {mutation.isPending ? '保存中…' : '确认修改'}
                    </Button>
                </>
            }
        >
            <form
                id="change-password-form"
                className="space-y-4"
                onSubmit={handleSubmit((values) => {
                    setTopError(null);
                    mutation.mutate(values);
                })}
                noValidate
            >
                {topError ? (
                    <div
                        role="alert"
                        className="rounded-md border border-[#E8C3BF] bg-bad-tint px-3 py-2 text-[13px] text-bad"
                    >
                        {topError}
                    </div>
                ) : null}

                <div>
                    <label
                        htmlFor="cp-current"
                        className="block text-[13px] font-medium text-ui-text"
                    >
                        当前密码
                    </label>
                    <input
                        id="cp-current"
                        type="password"
                        autoComplete="current-password"
                        className="studio-input mt-1.5"
                        {...register('current_password')}
                    />
                    {errors.current_password ? (
                        <p className="mt-1 text-[12px] text-bad">
                            {errors.current_password.message}
                        </p>
                    ) : null}
                </div>

                <div>
                    <label htmlFor="cp-new" className="block text-[13px] font-medium text-ui-text">
                        新密码
                    </label>
                    <input
                        id="cp-new"
                        type="password"
                        autoComplete="new-password"
                        className="studio-input mt-1.5"
                        {...register('new_password')}
                    />
                    <p className="mt-1 text-[12px] text-ui-sub">至少 12 个字节。</p>
                    {errors.new_password ? (
                        <p className="mt-1 text-[12px] text-bad">{errors.new_password.message}</p>
                    ) : null}
                </div>

                <div>
                    <label
                        htmlFor="cp-confirm"
                        className="block text-[13px] font-medium text-ui-text"
                    >
                        确认新密码
                    </label>
                    <input
                        id="cp-confirm"
                        type="password"
                        autoComplete="new-password"
                        className="studio-input mt-1.5"
                        {...register('confirm_password')}
                    />
                    {errors.confirm_password ? (
                        <p className="mt-1 text-[12px] text-bad">
                            {errors.confirm_password.message}
                        </p>
                    ) : null}
                </div>
            </form>
        </Modal>
    );
}
