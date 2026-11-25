# Silva API 接入指南

当前版本提供健康检查、身份认证、内置角色、账户管理、审计查询、媒体上传、文章与审核、分类标签、站点发布任务和 Public API。个人资料维护与 AI 采集接口尚未开放。

## OpenAPI 交付与 Apifox 导入

接口字段、约束和请求/响应示例统一维护在 [OpenAPI 文件](../../apps/api/openapi.json) 中。路径与方法遵守 [RESTful 接口设计规范](../contracts/restful-api.md)。成功与失败的响应结构、错误码登记表见 [API 响应结构规范](../contracts/api-response.md)。将 `server/apps/api/openapi.json` 作为 OpenAPI 3.1 文件导入 Apifox，进行预览与调试。

`info.version` 表示当前 API 发布版本；`openapi` 表示文档规范版本。变更接口时同步更新该文件，再重新导入 Apifox。

## 客户端认证

管理接口使用 JWT Bearer 认证。登录返回 `access_token` 与 `refresh_token`，之后在请求头 `Authorization: Bearer <access_token>` 中携带 access token；Apifox 中把认证方式设为 Bearer Token 并填入 access token。服务端不使用 Cookie、CSRF token 或 Origin 校验。

- access token 默认 900 秒过期（`SILVA_ACCESS_TOKEN_TTL_SECONDS`），过期后返回 401 `token_expired`，此时调用 `POST /api/v1/admin/tokens` 提交 `{"refresh_token": "..."}` 换取新令牌对，再重试原请求。
- refresh token 默认 7 天过期（`SILVA_REFRESH_TOKEN_TTL_SECONDS`），每次刷新都会轮换：旧 refresh token 立即失效，必须保存新返回的 refresh token。已用过的 refresh token 再次提交视为重放，整个会话被撤销，需要重新登录。
- 每次请求都按令牌中的会话读取账户当前状态、角色和权限。退出登录、账户停用、管理员重置密码或本人修改密码（其他会话）后，相关令牌立即返回 401 `unauthorized`。
- 令牌等同于凭据：不要写入日志、URL 或共享的调试环境；生产环境只通过 HTTPS 传输。生产入口应限制运维接口的外部访问。

## 登录、读取当前账户、刷新和退出

以下命令在 Bash 或 Zsh 中执行，需要 curl 和 jq。先按 [服务端 README](../../README.md) 初始化管理员。示例把令牌保存在仅当前用户可读的临时目录，结束时清理。

```sh
SILVA_API_BASE=https://admin.example.com
SILVA_API_TMP=$(mktemp -d)
chmod 700 "$SILVA_API_TMP"
trap 'rm -rf -- "$SILVA_API_TMP"' EXIT
export SILVA_API_EMAIL=admin@example.com
printf '登录密码: '
read -r -s SILVA_API_PASSWORD
printf '\n'
export SILVA_API_PASSWORD

jq -nc '{email: env.SILVA_API_EMAIL, password: env.SILVA_API_PASSWORD}' \
  | curl --fail-with-body --silent --show-error \
      "$SILVA_API_BASE/api/v1/admin/sessions" \
      -H 'Content-Type: application/json' \
      --data-binary @- > "$SILVA_API_TMP/login.json"
unset SILVA_API_PASSWORD
SILVA_API_ACCESS=$(jq -er '.data.access_token' "$SILVA_API_TMP/login.json")
SILVA_API_REFRESH=$(jq -er '.data.refresh_token' "$SILVA_API_TMP/login.json")

curl --fail-with-body --silent --show-error \
  "$SILVA_API_BASE/api/v1/admin/users/me" \
  -H "Authorization: Bearer $SILVA_API_ACCESS"
```

登录成功返回 201 Created，正文只含两个令牌及其过期时间：`{"data": {"access_token", "refresh_token", "access_token_expires_at", "refresh_token_expires_at"}}`，两个 `*_expires_at` 为北京时间（UTC+08:00）RFC 3339 时间，带显式偏移（如 `2026-11-24T16:15:00+08:00`），精确到秒，与令牌 `exp` 声明一致。`GET /users/me` 返回当前账户详情、角色与权限：`{"data": {"id", "email", "display_name", "status", "roles", "created_at", "updated_at", "last_login_at", "permissions"}}`。

access token 过期后刷新令牌，并同时替换两个变量：

```sh
jq -nc --arg token "$SILVA_API_REFRESH" '{refresh_token: $token}' \
  | curl --fail-with-body --silent --show-error \
      "$SILVA_API_BASE/api/v1/admin/tokens" \
      -H 'Content-Type: application/json' \
      --data-binary @- > "$SILVA_API_TMP/refresh.json"
SILVA_API_ACCESS=$(jq -er '.data.access_token' "$SILVA_API_TMP/refresh.json")
SILVA_API_REFRESH=$(jq -er '.data.refresh_token' "$SILVA_API_TMP/refresh.json")
```

刷新同样返回 201，正文与登录相同：`{"data": {"access_token", "refresh_token", "access_token_expires_at", "refresh_token_expires_at"}}`。同一 refresh token 并发提交时只有一次成功。

## 查询列表与更新账户

继续使用上一步的变量查询账户：

```sh
curl --fail-with-body --silent --show-error \
  "$SILVA_API_BASE/api/v1/admin/users?page=1&per_page=20" \
  -H "Authorization: Bearer $SILVA_API_ACCESS"
```

分页响应为 `{ "data": [...], "meta": { "pagination": { "page": 1, "per_page": 20, "total": 1, "total_pages": 1 } } }`。`page` 从 1 开始，默认 1；`per_page` 默认 20，范围 1–100。账户和审计列表按 `created_at DESC, id DESC` 排序。

将 `SILVA_API_USER_ID` 替换为列表中的实际账户 UUIDv7；更新会修改真实数据：

```sh
SILVA_API_USER_ID=019535d9-3df7-79fb-b466-fa907fa17f9e
curl --fail-with-body --silent --show-error \
  --request PATCH "$SILVA_API_BASE/api/v1/admin/users/$SILVA_API_USER_ID" \
  -H "Authorization: Bearer $SILVA_API_ACCESS" \
  -H 'Content-Type: application/json' \
  --data '{"display_name":"新显示名称"}'
```

更新时字段省略或显式传 `null` 均保持原值；`roles` 提供后会替换整组角色，不能是空数组或重复角色。`status` 可为 `active` 或 `disabled`，禁用账户会原子撤销该账户的全部会话，其令牌立即失效。后续请求按当前账户状态与权限授权。

登录与刷新之外的管理接口都要求有效的 access token。JSON 请求最大 64 KiB，输入不接受未知字段。密码长度按 UTF-8 字节计算，创建账户时为 12–1024 字节；显示名称最多 100 字节。

操作完成后删除当前会话（退出登录），成功返回 204 且没有响应体；该会话的 access token 和 refresh token 随即失效：

```sh
curl --fail-with-body --silent --show-error \
  --request DELETE "$SILVA_API_BASE/api/v1/admin/sessions/current" \
  -H "Authorization: Bearer $SILVA_API_ACCESS"
unset SILVA_API_ACCESS SILVA_API_REFRESH
```

## 媒体上传

用一个 `POST /api/v1/admin/media` 发送图片原始字节。支持 JPEG、PNG、WebP，最大 10 MiB，单边最多 16384 像素，总像素最多 4000 万。可选查询参数 `filename` 保留原文件名。使用文件字节数作为 `Content-Length`，浏览器发送 `File` 时会自动填写。

```ts
async function uploadImage(file: File, accessToken: string) {
  const query = new URLSearchParams({ filename: file.name });
  const response = await fetch(`/api/v1/admin/media?${query}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": file.type || "application/octet-stream",
    },
    body: file,
  });
  const result = await response.json();
  if (!response.ok) throw new Error(result.error.message);
  return result.data; // media.id 用于文章引用，url 可直接打开或作为 img.src
}
```

成功返回 201、媒体详情的 `Location`、已就绪的 `data.media` 和短期只读的 `data.url`。持有链接的人可在有效期内查看图片，默认 15 分钟。长期引用保存媒体 ID；后台可随时用 Bearer Token 读取 `GET /api/v1/admin/media/{id}/content`。替代文本使用 `PATCH /media/{id}` 提交 `{"alt_text":"..."}`，删除使用 `DELETE /media/{id}`。

## 文章、审核与发布

文章的状态由三个独立指针表达：当前编辑版本（`current_revision_id`）、期望公开版本（`publication.desired_published_revision_id`）和线上版本（`publication.live_revision_id`）。不要用单一状态字段推断文章是否已上线。

1. **创建**：`POST /api/v1/admin/posts` 提交 slug 与完整修订内容，返回文章详情（含第 1 条修订）。
2. **保存与自动保存**：`POST /posts/{id}/revisions` 提交完整内容和 `expected_revision_id`（编辑器读取时的 `current_revision_id`）。
   - 内容有变化时返回 201 和新修订，之后用它的 `id` 作为下一次的 `expected_revision_id`。
   - 内容与当前版本相同时返回 200 和当前版本，自动保存可以放心按固定间隔提交。
   - 返回 `409 revision_conflict` 时说明别人已经保存过：保留本地输入，重新读取文章，核对差异后再提交。
3. **送审**：作者 `POST /posts/{id}/reviews` 提交 `{"revision_id": "..."}`。每个修订只能送审一次；被拒绝后保存新修订再送审。
4. **审核**：Editor/Admin `PATCH /posts/{id}/reviews/{review_id}` 提交 `{"decision": "approved"}` 或 `rejected`，可附 `comment`。
5. **上线**：Editor/Admin `PUT /posts/{id}/publication` 提交已批准的 `revision_id` 和读取到的 `expected_publication_version`，返回 202 与 `publish_job_id`。**202 不代表已上线**：页面显示「待上线」，用 `GET /publishing/jobs/{id}` 轮询，`active` 表示已上线，`failed` 显示 `error_message`，`superseded` 表示被更新的目标取代（重新读取文章即可）。
6. **撤下**：`DELETE /posts/{id}/publication?expected_publication_version=N`，同样返回 202；新版本上线后文章在公开站点返回 404。slug 在首次上线后锁定，撤下也不解锁。

正文图片写作 `![说明](media:<媒体 ID>)`，只能引用已就绪且本人上传（或持有 `media.manage`）的媒体；原始 HTML 会按文本显示。修订返回的 `rendered_html` 中图片指向公开媒体地址，后台预览时把 `/api/v1/public/media/{id}` 替换为通过 `GET /api/v1/admin/media/{id}/content` 读取的 blob 地址。请求正文上限为 1 MiB，Markdown 正文最多 512 KiB。

分类与标签由 Editor/Admin 维护，所有内容角色都可以读取。修改已出现在线上文章中的分类或标签名称时，响应的 `publication` 给出新登记的发布任务。

## Public API

`/api/v1/public/*` 无需认证，只读取当前线上版本，每个响应带 `X-Site-Generation`。静态页面渲染时记录自己的 generation，之后的动态请求带上 `X-Expected-Generation`；返回 `409 site_generation_changed` 时清空页面中的旧数据并提示刷新，不要拼接两个版本的数据。站点尚未上线时返回 `503 site_not_ready`。

## 权限

| 接口 | 要求 |
| --- | --- |
| `POST /api/v1/admin/sessions`（登录）、`POST /api/v1/admin/tokens`（刷新） | 无需 access token |
| `GET /api/v1/admin/users/me`、`DELETE /api/v1/admin/sessions/current`（退出）、`PUT /api/v1/admin/users/me/password` | 有效 access token |
| `GET/POST /api/v1/admin/users`、`PATCH /api/v1/admin/users/{id}` | `users.manage` |
| `GET /api/v1/admin/roles` | `roles.read` |
| `GET /api/v1/admin/audit-logs` | `audit.read` |
| 文章读取、保存修订、送审 | `posts.manage` 可访问全部文章；只有 `posts.read_own`、`posts.edit_own`、`posts.submit_own` 时只能访问本人文章，他人文章返回 404 |
| 创建文章 | `posts.create` |
| 审核决定 / 上线与撤下 | `posts.review` / `posts.publish`（Editor、Admin） |
| 分类标签读取 / 维护 | 任一内容权限 / `taxonomy.manage`（Editor、Admin） |
| 发布任务查询 | `jobs.read` 可查看全部；只有 `jobs.read_own` 时只返回本人触发的任务 |
| 登记全站重建、重试失败任务 | `publishing.retry`（Admin） |
| `/api/v1/public/*` | 无需认证 |
| `/api/v1/admin/media` 下的全部接口 | `media.manage_own`（所有内置角色）只能看到和操作本人上传的媒体，他人媒体返回 404；`media.manage`（Editor、Admin）可操作全部媒体 |

内置角色为 `author`、`editor`、`admin`；Editor 包含 Author 权限，Admin 包含 Editor 权限。账户、角色和审计接口需要 Admin 对应权限。登录不会授予额外权限。OpenAPI 的 `x-required-permission` 记录每个接口的权限要求。

## 响应结构与排障

成功时 HTTP 状态为 2xx，正文为 `{"data": ...}`，分页列表另有 `meta.pagination`；204 没有正文。失败时正文统一为：

```json
{
  "error": {
    "code": "validation_error",
    "message": "Request validation failed",
    "details": [
      { "field": "display_name", "code": "out_of_range", "message": "display_name must contain 1 to 100 bytes" }
    ]
  }
}
```

客户端根据 HTTP 状态和 `error.code` 判断错误；`message` 用于排障，不作为判断条件。`details` 只在 422 中有内容，按 `field` 定位到具体输入。完整的错误码和字段错误码见 [API 响应结构规范](../contracts/api-response.md#5-错误码登记表)。

| HTTP 状态 | 常见错误码 | 处理 |
| --- | --- | --- |
| 400 | `malformed_json` | 检查请求体是否为合法 JSON |
| 401 | `token_expired` | 用 refresh token 调用 `POST /tokens` 后重试 |
| 401 | `invalid_credentials`、`unauthorized` | 检查凭据，或重新登录 |
| 403 | `forbidden` | 检查当前账户的权限 |
| 404 / 405 | `not_found` / `method_not_allowed` | 检查资源、路径或方法 |
| 409 | `email_in_use` | 更换已存在的邮箱 |
| 409 | `last_admin_required` | 至少保留一个启用状态的管理员，先为其他账户分配 admin 再停用或降级 |
| 409 | `slug_in_use` / `slug_locked` | 更换 slug / 文章已上线过，slug 不能再改 |
| 409 | `revision_conflict` | 重新读取文章，核对差异后再保存 |
| 409 | `review_already_submitted` / `review_not_pending` | 该修订已送审 / 审核已有决定 |
| 409 | `publication_conflict` / `revision_not_approved` / `already_unpublished` | 重新读取发布状态 / 先送审并批准 / 文章没有上线目标 |
| 409 | `taxonomy_in_use` | 分类或标签仍被修订引用，不能删除 |
| 409 | `publish_job_not_retryable` | 只能重试仍为最新目标的失败任务 |
| 409 | `site_generation_changed` | 线上版本已更新，清空旧数据并提示刷新 |
| 409 | `upload_incomplete` / `upload_expired` | 完成直传后重试确认 / 已超过截止时间，重新申请上传 |
| 409 | `media_not_ready` / `media_in_use` | 媒体尚未通过校验 / 先解除文章或个人资料对该媒体的引用 |
| 413 / 415 | `payload_too_large` / `unsupported_media_type` | 检查正文大小和 Content-Type |
| 422 | `validation_error` | 按 `details` 中的字段与字段错误码修正输入；路径 ID 不是 UUIDv7 时 `field` 为 `id` |
| 422 | `media_invalid` | 文件未通过校验，原因见错误消息；更换文件后重新上传 |
| 429 | `rate_limited` | 按 `Retry-After` 等待后重试 |
| 500 | `internal_error` | 记录请求标识并排查服务依赖 |
| 503 | `site_not_ready` | 站点尚未有线上版本，稍后重试 |
| 503 | `storage_unavailable` | 服务端未配置对象存储（仅开发环境），配置 `SILVA_STORAGE_*` 后重启 |

响应头 `X-Request-Id` 是服务端生成的 UUIDv7，排障时提供它；错误正文不重复携带。`X-Trace-Id` 是 32 位小写十六进制 W3C trace ID，用于链路查询，不是 UUID。

## 维护与检查

OpenAPI 文件是当前手工维护的唯一接口契约；修改接口时同步更新路径、Schema、权限、状态码、中文说明和示例，并更新契约测试。示例使用虚构账户和令牌，不填写真实凭据。

在 `server/` 下执行：

```sh
make api-docs-check
make check
SILVA_TEST_DATABASE_URL=postgres://silva:silva@127.0.0.1:55439/silva make test-db
```

`api-docs-check` 通过 Cargo 执行 Rust 契约测试，校验 OpenAPI 3.1 文档结构、Schema、示例、引用、路由和权限约束；同一组测试也包含在 `make check` 中。数据库集成测试创建隔离临时数据库，并按 OpenAPI Schema 校验真实响应。

未来引入 utoipa 时，统一迁移到代码生成，再由构建输出同一份契约；不要同时手工维护另一份接口定义。
