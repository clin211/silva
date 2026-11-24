# Silva API 接入指南

当前版本提供健康检查、身份认证、内置角色、账户管理、审计查询和媒体上传。CMS、发布与采集接口尚未开放。

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

图片不经过 API 上传：先向 API 申请，再由浏览器直传到私有对象存储（Cloudflare R2 或阿里云 OSS），最后请 API 校验。只接受 JPEG、PNG、WebP，单文件不超过 10 MiB，单边不超过 16384 像素且总像素不超过 4000 万。

1. `POST /api/v1/admin/media` 提交文件名、类型和大小，得到 `media`（`pending`）和 `upload` 直传说明。
2. 按 `upload.method` 把文件原始字节发到 `upload.url`，原样带上 `upload.headers`，**不要**带 `Authorization`。签名包含声明的大小，请求体必须正是第 1 步声明的那个文件，字节数不符时存储服务返回 403。
3. 在 `upload.expires_at`（默认 15 分钟）前调用 `PATCH /api/v1/admin/media/{id}` 提交 `{"status": "uploaded"}`。服务端按实际字节识别格式并完整解码，通过后返回 `status: ready`，记录里的 `mime_type`、`size_bytes`、`width`、`height` 是校验结果。

```ts
async function uploadImage(file: File, accessToken: string) {
  const auth = { Authorization: `Bearer ${accessToken}` };
  const created = await fetch("/api/v1/admin/media", {
    method: "POST",
    headers: { ...auth, "Content-Type": "application/json" },
    body: JSON.stringify({ filename: file.name, declared_mime_type: file.type, size_bytes: file.size }),
  }).then((response) => response.json());
  const { media, upload } = created.data;
  const put = await fetch(upload.url, { method: upload.method, headers: upload.headers, body: file });
  if (!put.ok) throw new Error(`直传失败：${put.status}`);
  const confirmed = await fetch(`/api/v1/admin/media/${media.id}`, {
    method: "PATCH",
    headers: { ...auth, "Content-Type": "application/json" },
    body: JSON.stringify({ status: "uploaded" }),
  });
  return confirmed.json(); // 422 media_invalid 时需重新申请上传
}
```

- **重复确认是安全的**：已就绪的媒体再次确认返回同一记录；已拒绝的返回同一个 `422 media_invalid`。网络中断后可以直接重试确认。
- **被拒绝后不能复用**：`rejected` 记录的拒绝原因见 `rejection_reason`，需重新调用 `POST /media` 申请新的上传。
- **替代文本**：`PATCH /media/{id}` 提交 `{"alt_text": "..."}` 修改，空字符串清除；可与确认放在同一个请求。
- **直传地址只用一次**：确认成功后不要再向同一地址上传；地址在 `expires_at` 前仍然有效，重复上传的文件不会被使用，只会由存储的生命周期规则删除。
- **显示图片**：`GET /media/{id}/content` 以流式响应返回文件，需要 `Authorization`，不能直接作为 `<img src>`。用 `fetch` 读取后 `URL.createObjectURL(await response.blob())` 显示，不再使用时调用 `URL.revokeObjectURL`。
- **删除**：`DELETE /media/{id}` 删除记录和文件；仍被文章修订或个人资料引用时返回 `409 media_in_use`。
- 过期未确认的上传在截止后约 1 小时由 Worker 删除，被拒绝的记录保留 7 天。

## 权限

| 接口 | 要求 |
| --- | --- |
| `POST /api/v1/admin/sessions`（登录）、`POST /api/v1/admin/tokens`（刷新） | 无需 access token |
| `GET /api/v1/admin/users/me`、`DELETE /api/v1/admin/sessions/current`（退出）、`PUT /api/v1/admin/users/me/password` | 有效 access token |
| `GET/POST /api/v1/admin/users`、`PATCH /api/v1/admin/users/{id}` | `users.manage` |
| `GET /api/v1/admin/roles` | `roles.read` |
| `GET /api/v1/admin/audit-logs` | `audit.read` |
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
| 409 | `upload_incomplete` / `upload_expired` | 完成直传后重试确认 / 已超过截止时间，重新申请上传 |
| 409 | `media_not_ready` / `media_in_use` | 媒体尚未通过校验 / 先解除文章或个人资料对该媒体的引用 |
| 413 / 415 | `payload_too_large` / `unsupported_media_type` | 检查正文大小和 Content-Type |
| 422 | `validation_error` | 按 `details` 中的字段与字段错误码修正输入；路径 ID 不是 UUIDv7 时 `field` 为 `id` |
| 422 | `media_invalid` | 文件未通过校验，原因见媒体的 `rejection_reason`；重新申请上传 |
| 429 | `rate_limited` | 按 `Retry-After` 等待后重试 |
| 500 | `internal_error` | 记录请求标识并排查服务依赖 |
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
