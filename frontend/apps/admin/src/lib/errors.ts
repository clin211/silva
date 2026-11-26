import type { Schemas } from '@silva/api-client';
import type { FieldPath, FieldValues, UseFormSetError } from 'react-hook-form';

export type ErrorCode = Schemas['ErrorCode'];
export type FieldError = Schemas['FieldError'];
export type FieldErrorCode = Schemas['FieldErrorCode'];
export type MediaRejectionReason = Schemas['MediaRejectionReason'];

export interface ApiErrorOptions {
    status: number;
    code?: string;
    message?: string;
    details?: FieldError[];
    requestId?: string | null;
    traceId?: string | null;
    retryAfter?: number | null;
}

export class ApiError extends Error {
    readonly status: number;
    readonly code: string;
    readonly serverMessage: string;
    readonly details: FieldError[];
    readonly requestId: string | null;
    readonly traceId: string | null;
    readonly retryAfter: number | null;

    constructor(options: ApiErrorOptions) {
        const userMessage = getErrorMessage(options.code, options.status, options.retryAfter);
        super(userMessage);
        this.name = 'ApiError';
        this.status = options.status;
        this.code = options.code ?? 'unknown_error';
        this.serverMessage = options.message ?? '';
        this.details = options.details ?? [];
        this.requestId = options.requestId ?? null;
        this.traceId = options.traceId ?? null;
        this.retryAfter = options.retryAfter ?? null;
    }
}

const ERROR_CODE_MESSAGES: Record<ErrorCode, string> = {
    malformed_json: '请求格式无效，请刷新页面后重试。',
    unauthorized: '登录状态已失效，请重新登录。',
    token_expired: '访问令牌已过期，正在刷新会话。',
    invalid_credentials: '邮箱或密码不正确，请检查后重试。',
    forbidden: '当前账户没有权限执行此操作。',
    not_found: '请求的资源不存在或无权访问。',
    method_not_allowed: '请求方法不受支持。',
    email_in_use: '该邮箱已被其他账户使用，请更换邮箱。',
    last_admin_required: '系统至少需要保留一名启用状态的管理员，请先为其他账户分配管理员角色。',
    upload_incomplete: '文件尚未完成上传，请重新上传。',
    upload_expired: '上传链接已过期，请重新上传文件。',
    media_not_ready: '该媒体尚未通过校验，无法查看内容或被引用。',
    media_in_use: '该媒体仍被文章修订（含历史修订）或个人资料引用，无法删除。',
    slug_in_use: '该 Slug 已被占用，请更换其他名称。',
    slug_locked: '文章首次被接受发布后 Slug 已锁定，一期不支持修改。',
    revision_conflict: '检测到有更新的修订，已停止自动保存。你的本地内容已保留。',
    review_already_submitted: '该修订已经提交过审核，无需重复提交。',
    review_not_pending: '该审核记录已有最终决定，请刷新最新状态。',
    publication_conflict: '文章的发布状态已被其他操作更新，请刷新后确认再提交。',
    revision_not_approved: '只有已批准的修订才能设为上线目标。',
    already_unpublished: '当前文章没有待上线或已上线的公开目标。',
    taxonomy_in_use: '该分类或标签仍被文章修订引用，请先调整相关文章后再删除。',
    publish_job_not_retryable: '只能重试仍为最新目标的失败发布任务。',
    site_generation_changed: '线上站点版本已更新，请刷新页面。',
    payload_too_large: '提交的内容体积超过服务端限制，请缩减后重试。',
    unsupported_media_type: '不支持的文件或请求内容类型。',
    validation_error: '部分输入不符合要求，请根据表单提示修改。',
    media_invalid: '图片未通过服务端校验，请更换符合要求的图片后重试。',
    rate_limited: '请求过于频繁或尝试次数过多，请稍后再试。',
    internal_error: '服务端发生内部错误，请稍后重试。',
    storage_unavailable: '对象存储服务暂未配置或不可用，媒体上传暂时不可用。',
    site_not_ready: '公开站点尚未完成首次上线。',
};

const FIELD_ERROR_CODE_MESSAGES: Record<FieldErrorCode, string> = {
    required: '此项为必填项',
    unknown_field: '包含不受支持的字段',
    invalid_type: '输入类型不正确',
    invalid_format: '格式不符合要求',
    out_of_range: '长度或数值超出允许范围',
    invalid_value: '所选值不在允许范围内',
    duplicate: '存在重复项',
    incorrect: '输入与当前记录不符',
    unchanged: '新值必须与原值不同',
};

const MEDIA_REJECTION_MESSAGES: Record<MediaRejectionReason, string> = {
    too_large: '文件体积超过 10 MiB 限制。',
    unsupported_format: '仅支持 JPEG、PNG 或 WebP 格式的图片。',
    undecodable: '图片文件无法解码或已损坏，请重新导出后再上传。',
    dimensions_exceeded: '图片尺寸超出限制（单边最大 16384 像素，总像素不超过 4000 万）。',
};

export function getMediaRejectionMessage(reason: MediaRejectionReason | null | undefined): string {
    if (!reason) return '未通过图片校验。';
    return MEDIA_REJECTION_MESSAGES[reason] ?? '未通过图片校验。';
}

export function getFieldErrorMessage(detail: FieldError): string {
    return FIELD_ERROR_CODE_MESSAGES[detail.code] ?? '输入不符合要求';
}

export function getErrorMessage(
    code: string | undefined,
    status: number,
    retryAfter?: number | null,
): string {
    if (code === 'rate_limited' && retryAfter && retryAfter > 0) {
        if (retryAfter >= 60) {
            const mins = Math.ceil(retryAfter / 60);
            return `尝试次数过多或请求受限，请在约 ${mins} 分钟后再试。`;
        }
        return `请求受限，请在 ${retryAfter} 秒后重试。`;
    }
    if (code && code in ERROR_CODE_MESSAGES) {
        return ERROR_CODE_MESSAGES[code as ErrorCode];
    }
    if (status === 0) return '网络连接失败，请检查网络或后端服务状态。';
    if (status === 401) return ERROR_CODE_MESSAGES.unauthorized;
    if (status === 403) return ERROR_CODE_MESSAGES.forbidden;
    if (status === 404) return ERROR_CODE_MESSAGES.not_found;
    if (status === 409) return '当前状态发生冲突，请刷新后重试。';
    if (status === 413) return ERROR_CODE_MESSAGES.payload_too_large;
    if (status === 422) return ERROR_CODE_MESSAGES.validation_error;
    if (status === 429) return ERROR_CODE_MESSAGES.rate_limited;
    if (status >= 500) return ERROR_CODE_MESSAGES.internal_error;
    return '请求未能完成，请稍后重试。';
}

export function isApiError(error: unknown): error is ApiError {
    return error instanceof ApiError;
}

export function formatErrorWithRequestId(error: unknown): string {
    if (isApiError(error)) {
        if (error.status >= 500 && error.requestId) {
            return `${error.message}（Request ID: ${error.requestId}）`;
        }
        return error.message;
    }
    if (error instanceof Error && error.message) {
        return error.message;
    }
    return '发生未知错误，请稍后重试。';
}

/**
 * 将 422 validation_error 的 details 映射到 React Hook Form 字段错误。
 * 返回未能映射到具体表单字段的错误列表（以便在表单顶部兜底提示）。
 */
export function applyApiFieldErrors<TFieldValues extends FieldValues>(
    error: unknown,
    setError: UseFormSetError<TFieldValues>,
    fieldMap?: Record<string, FieldPath<TFieldValues>>,
): FieldError[] {
    if (!isApiError(error) || error.details.length === 0) {
        return [];
    }
    const unmatched: FieldError[] = [];
    for (const detail of error.details) {
        const mapped = fieldMap?.[detail.field] ?? (detail.field as FieldPath<TFieldValues>);
        if (!detail.field) {
            unmatched.push(detail);
            continue;
        }
        try {
            setError(mapped, {
                type: detail.code,
                message: getFieldErrorMessage(detail),
            });
        } catch {
            unmatched.push(detail);
        }
    }
    return unmatched;
}
