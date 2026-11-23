# API 响应结构规范

本规范规定 Silva 服务端 HTTP API 成功和失败时的响应结构、状态码选择和错误码管理方式。所有 `/api/*` 接口都必须遵守。机器可读的契约在 [OpenAPI 文件](../../apps/api/openapi.json) 中，与本文冲突时应修正其中一方，不允许两者长期不一致。

## 1. 参考与取舍

| 来源 | 做法 | 本项目采纳 |
| --- | --- | --- |
| Meta Graph API | 错误为 `{"error": {"message", "type", "code", "fbtrace_id"}}`；列表为 `{"data": [...], "paging": {...}}` | 错误外包一层 `error` 对象；带追踪 ID；列表数据放 `data`，分页信息与数据分开 |
| X API v2 | 成功为 `{"data", "includes", "meta"}`，`meta` 放 `result_count` 和翻页令牌 | 成功统一使用 `data`，非资源信息放 `meta` |
| GitHub REST | 资源直接返回；错误为 `{"message", "errors": [{"resource", "field", "code"}]}`；非法 JSON 返回 400，字段校验失败返回 422 | 字段级错误数组（`field` + `code`）；400 与 422 的区分 |
| Google AIP-193 / JSON Style Guide | `{"error": {"code", "message", "details"}}`，`data` 与 `error` 不同时出现，HTTP 状态是权威结果 | 顶层 `data` / `error` 互斥；不在正文重复状态码 |
| Microsoft REST API Guidelines | `{"error": {"code", "message", "details", "target"}}`，错误码为稳定字符串 | 稳定字符串错误码，`message` 不作为分支条件 |
| RFC 9457 Problem Details | `application/problem+json`，`type` / `title` / `status` / `detail` | 不采用。它要求为每类错误维护 URI，且对字段级错误没有统一格式；`{"error": {...}}` 更便于前端统一处理 |

另外明确不采用国内常见的 `{"code": 0, "msg": "", "data": {}}` 包装：它把错误编码到 200 响应里，网关、监控、缓存和重试策略都无法依据 HTTP 状态工作。

## 2. 总原则

1. **HTTP 状态码是请求结果的唯一依据。** 2xx 表示成功，4xx 表示调用方需要修正请求，5xx 表示服务端问题。正文不出现 `success`、`status`、`code: 0` 之类的字段。
2. **顶层只有一个键表示结果。** 成功正文必有 `data`，失败正文必有 `error`，两者不会同时出现。成功正文可以另带 `meta`，除此之外没有其他顶层键。
3. **JSON 约定：** `Content-Type: application/json`，UTF-8；字段名用 `snake_case`；时间为 RFC 3339 UTC 字符串（如 `2026-11-21T08:00:00Z`）；实体 ID 为带连字符的 UUIDv7 字符串；枚举值用小写 `snake_case` 字符串。
4. **字段稳定存在。** 资源 Schema 中声明的字段总是出现，没有值时返回 `null`，不省略。数组没有元素时返回 `[]`，不返回 `null`。
5. **不泄露内部细节。** 响应中不出现 SQL、堆栈、内部主机名、密码哈希或令牌原文；500 的细节只写入日志，用 `request_id` 关联。
6. **客户端必须容忍新增内容。** 客户端忽略未知字段；遇到未知错误码时按 HTTP 状态兜底处理。

## 3. 成功响应

### 3.1 状态码与正文

| 状态 | 适用场景 | 正文 |
| --- | --- | --- |
| 200 OK | 读取单个资源、读取集合、更新资源、返回结果的动作（如登录） | `{"data": ...}` 或 `{"data": [...], "meta": {...}}` |
| 201 Created | 新建资源 | `{"data": <新建的资源>}` |
| 202 Accepted | 已受理的异步任务（预留，目前没有接口使用） | `{"data": <任务资源>}` |
| 204 No Content | 动作成功且没有可返回的内容，如退出登录、修改密码 | 无正文 |

### 3.2 单个资源或动作结果

`data` 是一个对象：

```json
{
  "data": {
    "id": "019535d9-3df7-79fb-b466-fa907fa17f9e",
    "email": "author@example.com",
    "display_name": "示例作者",
    "status": "active",
    "roles": ["author"],
    "created_at": "2026-11-21T08:00:00Z",
    "updated_at": "2026-11-21T08:00:00Z",
    "last_login_at": null
  }
}
```

动作结果也是对象，例如登录返回 `{"data": {"user": {...}, "permissions": [...], "csrf_token": "..."}}`。

### 3.3 集合

集合的 `data` 始终是数组。分页集合在 `meta.pagination` 中返回分页信息：

```json
{
  "data": [{ "id": "019535d9-3df7-79fb-b466-fa907fa17f9e", "...": "..." }],
  "meta": {
    "pagination": { "page": 1, "per_page": 20, "total": 1, "total_pages": 1 }
  }
}
```

- `page` 从 1 开始，默认 1；`per_page` 默认 20，范围 1–100；`total_pages` 在没有数据时为 0。
- 页码超出范围不是错误，返回空的 `data`。
- 排序必须稳定：业务排序字段之后用 `id` 打破同分。
- 未分页的小型集合（如内置角色）只返回 `{"data": [...]}`，不带 `meta`。
- 以后引入游标分页时，在 `meta.pagination` 中增加游标字段，`data` 的结构保持不变。

### 3.4 不使用信封的端点

以下端点供基础设施使用，按各自生态的惯例返回，不套 `data`：

| 端点 | 正文 |
| --- | --- |
| `GET /health/live`、`GET /health/ready` | 裸 JSON，如 `{"status": "ok"}`；失败时仍返回第 4 节的错误结构 |
| `GET /metrics` | Prometheus 文本格式 |
| `GET /openapi.json` | OpenAPI 文档本身 |

需要登录的 `GET /api/v1/admin/system/health` 属于业务接口，返回 `{"data": {"status": "ok", "database": "up"}}`。

## 4. 失败响应

### 4.1 结构

所有 4xx 和 5xx（包括路由不存在、方法不允许、限流）都返回同一结构：

```json
{
  "error": {
    "code": "validation_error",
    "message": "Request validation failed",
    "details": [
      {
        "field": "roles[1]",
        "code": "duplicate",
        "message": "role is listed more than once"
      }
    ],
    "request_id": "019535d9-3df7-79fb-b466-fa907fa17f9e"
  }
}
```

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `code` | string | 稳定的错误码，取值见 5.1。客户端只根据 HTTP 状态和这个字段做判断 |
| `message` | string | 面向开发者的英文说明，措辞可能调整。不要用它做判断，也不要直接展示给终端用户；界面文案由前端按 `code` 提供 |
| `details` | array | 字段级错误，元素结构见 4.2。没有时为 `[]`，这个字段始终存在 |
| `request_id` | string | 服务端生成的 UUIDv7，与响应头 `X-Request-Id` 相同。用户反馈问题时提供它 |

### 4.2 字段级错误

`details` 目前只出现在 `validation_error` 中，服务端会尽量一次列出所有能发现的字段问题。

| 字段 | 说明 |
| --- | --- |
| `field` | 出错位置。请求体字段用点号和下标表示，如 `email`、`roles[1]`、`profile.name`；查询参数和路径参数直接用参数名，如 `per_page`、`id`；空字符串 `""` 表示整个请求体或整个查询串（例如正文是数组而不是对象） |
| `code` | 字段错误码，取值见 5.2 |
| `message` | 面向开发者的英文说明，不包含用户提交的值 |

前端表单可以按 `field` 把错误挂到对应的输入框，按 `code` 选择本地化文案。

## 5. 错误码登记表

错误码在代码中集中登记（见第 8 节），每个错误码只对应一个 HTTP 状态。

### 5.1 错误码（`error.code`）

| 状态 | 错误码 | 含义 | 客户端处理 |
| --- | --- | --- | --- |
| 400 | `malformed_json` | 请求体不是合法 JSON（语法错误、空正文、结尾有多余内容） | 修正请求体编码 |
| 401 | `unauthorized` | 会话缺失、无效、过期、已撤销，或账户已停用 | 跳转登录 |
| 401 | `invalid_credentials` | 登录时邮箱或密码错误。为防止枚举账户，不区分具体原因 | 提示重新输入 |
| 403 | `forbidden` | 已登录，但缺少所需权限 | 提示无权限 |
| 403 | `origin_rejected` | 写操作的 `Origin` 与 `SILVA_ADMIN_ORIGIN` 不完全一致 | 检查部署和代理配置 |
| 403 | `csrf_rejected` | `X-CSRF-Token` 缺失，或与当前会话不匹配 | 调用 `auth/me` 重新获取 Token 后重试 |
| 404 | `not_found` | 路径或资源不存在 | 提示资源不存在 |
| 405 | `method_not_allowed` | 路径存在，但不支持该 HTTP 方法 | 修正调用代码 |
| 409 | `email_in_use` | 规范化后的邮箱已被其他账户使用 | 提示更换邮箱 |
| 409 | `last_admin_required` | 操作会导致系统中没有启用状态的管理员 | 先给其他账户分配 admin |
| 413 | `payload_too_large` | 请求体超过 64 KiB | 缩小请求 |
| 415 | `unsupported_media_type` | `Content-Type` 不是 `application/json` 或 `application/*+json` | 修正请求头 |
| 422 | `validation_error` | 输入不满足约束，`details` 中列出具体字段 | 按 `details` 标注表单 |
| 429 | `rate_limited` | 超过请求频率限制 | 按 `Retry-After` 秒数等待后重试 |
| 500 | `internal_error` | 服务端内部错误 | 展示通用错误和 `request_id` |
| 503 | `service_unavailable` | 数据库等必要依赖不可用 | 稍后重试 |

[后端技术方案](../../../docs/03-backend-technical.md)中规划的 `revision_conflict`、`site_generation_changed`、`site_not_ready` 等错误码，要等对应接口实现时再按第 8 节的流程登记。

### 5.2 字段错误码（`error.details[].code`）

| 字段错误码 | 含义 | 示例 |
| --- | --- | --- |
| `required` | 缺少必填字段 | 登录时没有 `password` |
| `unknown_field` | 出现未定义的字段（所有输入都拒绝未知字段） | 创建账户时多传 `is_admin` |
| `invalid_type` | JSON 类型不符，或查询参数不是整数 | `"email": 1`、`page=abc` |
| `invalid_format` | 格式不符 | 邮箱格式错误；路径 `id` 不是 UUIDv7 |
| `out_of_range` | 长度、数量或数值超出范围 | 密码少于 12 字节；`per_page=500`；`roles` 为空数组 |
| `invalid_value` | 不在允许的取值集合内 | `status: "deleted"`；未知角色 |
| `duplicate` | 集合或查询参数中出现重复 | `roles: ["author", "author"]`；`page=1&page=2` |
| `incorrect` | 与服务端保存的值不符 | 修改密码时当前密码错误 |
| `unchanged` | 新值必须与原值不同 | 新密码与当前密码相同 |

## 6. 状态码选择规则

- **400 和 422：** 请求体无法解析为 JSON 时返回 400 `malformed_json`；JSON 合法，但字段缺失、多余、类型或取值不符时返回 422 `validation_error`。查询参数和路径参数无效时也返回 422。
- **401 和 403：** 无法确认调用者身份时返回 401；身份已确认但不允许执行该操作时返回 403。Origin 和 CSRF 校验失败属于 403。
- **404：** 资源不存在时返回 404。调用者无权知道某资源是否存在时，也返回 404 而不是 403。
- **409：** 请求本身合法，但与资源当前状态冲突，例如唯一性约束、并发版本或业务不变量。每种冲突使用单独的错误码。
- **422 中的业务规则：** 只涉及请求自身字段的规则（如“新密码不能与当前密码相同”）返回 422，并在 `details` 中指明字段；需要依赖其他资源状态的规则返回 409。
- **429：** 必须带 `Retry-After` 响应头。
- **500 和 503：** 依赖暂时不可用、调用方重试可能成功时返回 503；其他未预期的错误返回 500。

## 7. 响应头

| 响应头 | 出现位置 | 说明 |
| --- | --- | --- |
| `X-Request-Id` | 所有响应 | 服务端生成的 UUIDv7，与错误正文的 `request_id` 相同 |
| `X-Trace-Id` | 所有响应 | 32 位小写十六进制 W3C trace ID，用于链路查询 |
| `Cache-Control: no-store` | 所有响应 | 管理接口的响应不允许缓存 |
| `X-Content-Type-Options: nosniff` | 所有响应 | 禁止浏览器猜测内容类型 |
| `Retry-After` | 429 | 需要等待的秒数 |
| `Set-Cookie` | 登录、退出 | 会话 Cookie，属性见 [身份契约](identity.md) |

## 8. 代码落地

| 位置 | 职责 |
| --- | --- |
| `apps/api/src/response.rs` | 构造成功响应：`ok`（200 + `data`）、`created`（201）、`page`（200 + `data` + `meta.pagination`）、`no_content`（204）。业务处理器只通过这些函数返回 2xx |
| `apps/api/src/error.rs` | `ErrorCode` 登记全部对外错误码，并通过 `status()` 决定 HTTP 状态；`ApiError` 负责序列化错误正文，并从任务上下文读取 `request_id` |
| `crates/shared/src/error.rs` | 各层共用的 `AppError`、`Conflict`（每个变体对应一个 409 错误码）、`FieldError` 和 `FieldErrorCode`。领域层和应用层只表达“哪里出了什么问题”，不涉及 HTTP |
| `apps/api/src/http.rs` | `Input`（JSON 请求体）、`PageInput`（分页参数）、`IdPath`（UUIDv7 路径参数）提取器，把解析失败转换为上述错误 |

### 8.1 新增接口

1. 成功响应使用 `response::ok`、`created`、`page` 或 `no_content`，不要直接返回 `Json(...)`。
2. JSON 请求体使用 `Input<T>`，输入结构体加 `#[serde(deny_unknown_fields)]`。
3. 校验失败时返回 `AppError::Validation`，在 `field` 中写明具体字段；一次能发现多个问题时全部列出。
4. 在 OpenAPI 中：成功响应引用 `{"data": ...}` 包装的 Schema；有 JSON 请求体的接口声明 `400`（引用 `#/components/responses/MalformedJson`）；每个错误响应声明 `x-error-codes` 和示例。

### 8.2 新增错误码

1. 在 `ErrorCode` 中增加变体，并同时更新 `ALL` 和 `status()`；如果是 409，先在 `Conflict` 中增加业务原因。
2. 更新 OpenAPI 中 `ErrorCode` 的 `enum` 和说明，以及相关接口的 `x-error-codes`。
3. 更新本文第 5 节的登记表和 [API 接入指南](../api/README.md)。
4. 新增字段错误码时，同样更新 `FieldErrorCode`、OpenAPI 的 `FieldErrorCode` 和 5.2 节。

新增错误码是向后兼容的变更；删除或重命名错误码、改变已有错误码对应的状态码，属于不兼容变更，需要新的 API 版本。

## 9. 自动校验

- `make api-docs-check` 中的契约测试会校验以下内容：OpenAPI 中 `ErrorCode` / `FieldErrorCode` 与代码登记表一致；所有 `/api/*` 成功响应的顶层只有 `data`（必有）和 `meta`；所有错误响应使用 `ErrorResponse`，并且声明的错误码与 HTTP 状态一致；有 JSON 请求体的接口都声明了 400。
- 同一组测试在不连接数据库的情况下，验证非法 JSON、错误的 Content-Type、缺失字段、未知字段、类型错误和分页越界时返回的状态、错误码和 `details`，并确认 `details` 中不回显用户提交的值。
- `make test-db` 中的集成测试按 OpenAPI Schema 校验真实接口的每个响应，并对各类校验失败断言具体的 `field` 和 `code`。
