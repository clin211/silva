# Silva API 接入指南

当前版本提供健康检查、身份认证、内置角色、账户管理和审计查询。CMS、媒体、发布与采集接口尚未开放。

## OpenAPI 交付与 Apifox 导入

接口字段、约束和请求/响应示例统一维护在 [OpenAPI 文件](../../apps/api/openapi.json) 中。成功与失败的响应结构、错误码登记表见 [API 响应结构规范](../contracts/api-response.md)。将 `server/apps/api/openapi.json` 作为 OpenAPI 3.1 文件导入 Apifox，进行预览与调试。

运行服务后，也可从 `/openapi.json` 下载当前运行版本的契约：

```sh
curl --fail --output silva-openapi.json http://127.0.0.1:8088/openapi.json
```

`info.version` 表示当前 API 发布版本；`openapi` 表示文档规范版本。变更接口时同步更新该文件，再重新导入 Apifox。

## 客户端认证

在 Apifox 中将请求地址设置为目标环境，登录及管理写操作显式携带与 `SILVA_ADMIN_ORIGIN` 精确匹配的 `Origin`。例如配置为 `https://admin.example.com` 时，请求头也使用该值，不带尾部斜杠。

登录成功后保存并回传 `__Host-silva_session` Cookie，并从登录响应的 `data.csrf_token` 字段取得 Token。登录之外的管理写操作还要携带 `X-CSRF-Token`；可通过 `GET /api/v1/admin/auth/me` 重新获取当前会话的 Token。Apifox 中的 Cookie 和 Token 传递由调试配置维护。

会话 Cookie 具有 HttpOnly、Secure、SameSite=Lax、Path=/ 属性且没有 Domain。请使用 HTTPS 入口调试，以便客户端按 Secure 属性回传 Cookie。服务端不接受 Bearer Token。

浏览器后台仍需通过同源 HTTPS 入口代理 `/api/v1/admin/*`，并将该入口的 Origin 配置为 `SILVA_ADMIN_ORIGIN`；服务端没有启用凭据式 CORS。生产入口应限制 `/openapi.json` 和运维接口的外部访问。

## 登录、读取会话和退出

以下命令在 Bash 或 Zsh 中执行，需要 curl、jq 和可信的 HTTPS 入口。先按 [服务端 README](../../README.md) 初始化管理员。示例只在临时目录保存 Cookie 和登录结果，结束时清理。

```sh
SILVA_API_BASE=https://admin.example.com
SILVA_API_ORIGIN=https://admin.example.com
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
      "$SILVA_API_BASE/api/v1/admin/auth/login" \
      -H 'Content-Type: application/json' \
      -H "Origin: $SILVA_API_ORIGIN" \
      --cookie-jar "$SILVA_API_TMP/cookies" \
      --data-binary @- > "$SILVA_API_TMP/login.json"
unset SILVA_API_PASSWORD
SILVA_API_CSRF=$(jq -er '.data.csrf_token' "$SILVA_API_TMP/login.json")

curl --fail-with-body --silent --show-error \
  "$SILVA_API_BASE/api/v1/admin/auth/me" \
  --cookie "$SILVA_API_TMP/cookies"
```

登录响应为 `{"data": {"user", "permissions", "csrf_token"}}`，并通过 `Set-Cookie` 设置会话。CSRF Token 绑定当前会话，不能混用不同登录会话的 Token。

## 查询列表与更新账户

继续使用上一步的变量查询账户：

```sh
curl --fail-with-body --silent --show-error \
  "$SILVA_API_BASE/api/v1/admin/users?page=1&per_page=20" \
  --cookie "$SILVA_API_TMP/cookies"
```

分页响应为 `{ "data": [...], "meta": { "pagination": { "page": 1, "per_page": 20, "total": 1, "total_pages": 1 } } }`。`page` 从 1 开始，默认 1；`per_page` 默认 20，范围 1–100。账户和审计列表按 `created_at DESC, id DESC` 排序。

将 `SILVA_API_USER_ID` 替换为列表中的实际账户 UUIDv7；更新会修改真实数据：

```sh
SILVA_API_USER_ID=019535d9-3df7-79fb-b466-fa907fa17f9e
curl --fail-with-body --silent --show-error \
  --request PATCH "$SILVA_API_BASE/api/v1/admin/users/$SILVA_API_USER_ID" \
  --cookie "$SILVA_API_TMP/cookies" \
  -H 'Content-Type: application/json' \
  -H "Origin: $SILVA_API_ORIGIN" \
  -H "X-CSRF-Token: $SILVA_API_CSRF" \
  --data '{"display_name":"新显示名称"}'
```

更新时字段省略或显式传 `null` 均保持原值；`roles` 提供后会替换整组角色，不能是空数组或重复角色。`status` 可为 `active` 或 `disabled`，禁用账户会原子撤销该账户的全部会话。后续请求按当前账户状态与权限授权。

所有管理写操作（POST、PATCH）要求精确匹配的 Origin；登录之外还要求当前会话的 CSRF Token。JSON 请求最大 64 KiB，输入不接受未知字段。密码长度按 UTF-8 字节计算，创建账户时为 12–1024 字节；显示名称最多 100 字节。

操作完成后退出登录，成功返回 204 且没有响应体：

```sh
curl --fail-with-body --silent --show-error \
  --request POST "$SILVA_API_BASE/api/v1/admin/auth/logout" \
  --cookie "$SILVA_API_TMP/cookies" \
  --cookie-jar "$SILVA_API_TMP/cookies" \
  -H "Origin: $SILVA_API_ORIGIN" \
  -H "X-CSRF-Token: $SILVA_API_CSRF"
unset SILVA_API_CSRF
```

## 权限

| 接口 | 要求 |
| --- | --- |
| `POST /api/v1/admin/auth/login` | 无需已有会话，要求 Origin |
| `GET /api/v1/admin/auth/me`、`POST /api/v1/admin/auth/logout`、`POST /api/v1/admin/auth/password` | 有效会话 |
| `GET/POST /api/v1/admin/users`、`PATCH /api/v1/admin/users/{id}` | `users.manage` |
| `GET /api/v1/admin/roles` | `roles.read` |
| `GET /api/v1/admin/audit-logs` | `audit.read` |
| `GET /api/v1/admin/system/health` | `system.read` |

内置角色为 `author`、`editor`、`admin`；Editor 包含 Author 权限，Admin 包含 Editor 权限。目前账户、角色、审计和系统健康接口需要 Admin 对应权限。登录不会授予额外权限。OpenAPI 的 `x-required-permission` 记录每个接口的权限要求。

## 响应结构与排障

成功时 HTTP 状态为 2xx，正文为 `{"data": ...}`，分页列表另有 `meta.pagination`；204 没有正文。失败时正文统一为：

```json
{
  "error": {
    "code": "validation_error",
    "message": "Request validation failed",
    "details": [
      { "field": "display_name", "code": "out_of_range", "message": "display_name must contain 1 to 100 bytes" }
    ],
    "request_id": "019535d9-3df7-79fb-b466-fa907fa17f9e"
  }
}
```

客户端根据 HTTP 状态和 `error.code` 判断错误；`message` 用于排障，不作为判断条件。`details` 只在 422 中有内容，按 `field` 定位到具体输入。完整的错误码和字段错误码见 [API 响应结构规范](../contracts/api-response.md#5-错误码登记表)。

| HTTP 状态 | 常见错误码 | 处理 |
| --- | --- | --- |
| 400 | `malformed_json` | 检查请求体是否为合法 JSON |
| 401 | `invalid_credentials`、`unauthorized` | 检查凭据，或重新登录 |
| 403 | `origin_rejected` | 检查页面 Origin 与服务端配置是否精确一致 |
| 403 | `csrf_rejected` | 从当前会话的 `auth/me` 重新获取 Token |
| 403 | `forbidden` | 检查当前账户的权限 |
| 404 / 405 | `not_found` / `method_not_allowed` | 检查资源、路径或方法 |
| 409 | `email_in_use` | 更换已存在的邮箱 |
| 409 | `last_admin_required` | 至少保留一个启用状态的管理员，先为其他账户分配 admin 再停用或降级 |
| 413 / 415 | `payload_too_large` / `unsupported_media_type` | 检查正文大小和 Content-Type |
| 422 | `validation_error` | 按 `details` 中的字段与字段错误码修正输入；路径 ID 不是 UUIDv7 时 `field` 为 `id` |
| 429 | `rate_limited` | 按 `Retry-After` 等待后重试 |
| 500 / 503 | `internal_error` / `service_unavailable` | 记录请求标识并排查服务依赖 |

响应头 `X-Request-Id` 是服务端生成的 UUIDv7，与错误体 `request_id` 一致。`X-Trace-Id` 是 32 位小写十六进制 W3C trace ID，用于链路查询，不是 UUID。

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
