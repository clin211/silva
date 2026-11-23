# Identity / RBAC 契约

来源：`docs/01-product.md` §3；`docs/03-backend-technical.md` §3、§5、§9。此契约固定第一里程碑认证与账户管理的验收边界。业务内容、媒体与发布用例在对应里程碑实现。

## 会话和输入边界

- 密码使用 Argon2id；数据库不保存明文密码。
- Session Cookie 固定要求 `HttpOnly; Secure; SameSite=Lax; Path=/`，host-only，不设置 `Domain`。数据库只保存会话 token hash。
- `POST /api/v1/admin/auth/login` 是无需既有会话的入口，只接受 JSON 和配置的**精确同源 Origin**。缺失、不匹配 Origin 均拒绝；不启用管理 API 跨域凭证。
- 其他管理写请求必须同时校验精确 Origin 和当前会话绑定的 `X-CSRF-Token`。CSRF token 由登录和 `GET /api/v1/admin/auth/me` 返回；另一个会话的 token 不可复用。
- `POST /api/v1/admin/auth/logout` 撤销当前会话，成功 204，无响应体。
- `GET /api/v1/admin/auth/me` 在 `data` 中返回当前用户、当前权限、`csrf_token`。
- 无有效会话返回 401；无权限或 CSRF 检查失败返回 403。
- 停用账户立即撤销其全部会话；每次授权使用当前账户状态与当前角色，不能依赖登录时权限快照。
- 认证响应与非公开预览使用 `Cache-Control: no-store`。登录和 API 均需限流，管理操作需审计，不记录密码、原始 session token 或敏感凭据。

## 角色与归属

| 角色 | 权限边界 |
| --- | --- |
| Visitor | 无需登录，只读已上线公开内容。 |
| Author | 创建文章、读取和编辑自己的文章及修订、上传和引用自己的已校验媒体、提交审核。不能审核、发布或取消发布。 |
| Editor | 管理所有文章及媒体、审核指定修订、发布/取消发布、维护分类标签及资源目录、调整 AI 展示、查看相关任务。 |
| Admin | 包含 Editor 权限；额外管理账户、分配内置角色、触发采集、重试发布、查看审计及系统状态。 |

- 即使只有一个管理员，也保留 `users`、`roles`、`permissions`、`user_roles`、`role_permissions`。
- 允许 Editor/Admin 审核自己编写的修订，但必须显式审核；不能隐式跳过审批。
- 服务端逐管理用例执行 RBAC 和资源归属授权；包含嵌套修订读取、上传确认、媒体引用。隐藏前端按钮不算授权。
- 只提供内置角色分配、账户创建与停用，不提供公开注册或自定义角色编辑。

## 第一里程碑管理端点

以下路径相对于 `/api/v1/admin`：

| 方法 | 路径 | 授权及结果 |
| --- | --- | --- |
| POST | `/auth/login` | JSON + 精确 Origin；返回会话 Cookie 与 CSRF token。 |
| POST | `/auth/logout` | 有效会话 + Origin + CSRF；204。 |
| GET | `/auth/me` | 有效会话；用户、权限、CSRF token。 |
| POST | `/auth/password` | 有效会话 + Origin + CSRF；校验当前密码后修改，204；保留当前会话并撤销其他会话。 |
| GET, POST | `/users` | Admin；分页列表、创建账户。 |
| PATCH | `/users/{id}` | Admin；启用/停用及分配内置角色。 |
| GET | `/roles` | Admin；内置角色。 |
| GET | `/audit-logs` | Admin；审计记录。 |
| GET | `/system/health` | Admin；系统健康状态。 |

以下为主线程根据原文边界冻结的第一里程碑实现约定（原文没有指定这些名称和阈值）：

- Cookie 名称：`__Host-silva_session`。
- 登录输入：`{email,password}`；登录和 `auth/me` 返回 `{"data":{user,permissions,csrf_token}}`。
- 创建账户：`{email,display_name,password,roles:["author"|"editor"|"admin"]}`，201 直接返回 User。
- 修改账户：`{display_name?,status?:"active"|"disabled",roles?:[...],password?}`，200 直接返回 User。`password` 为管理员重置，设置后撤销该账户全部会话。
- 修改自己的密码：`{current_password,new_password}`；当前密码错误、新密码不符合 12..1024 字节或与当前密码相同时返回 422 `validation_error`，`details` 分别指向 `current_password`（`incorrect`）或 `new_password`（`out_of_range`、`unchanged`）；与登录共用每 IP 登录限流额度。角色必须是非空、无重复的内置键数组。
- 新账户密码为 12..1024 UTF-8 字节；用户邮箱去首尾空白后转小写，最大 254 字节；显示名去空白后非空，最大 100 字节。
- Origin 失败：403 `origin_rejected`；CSRF 失败：403 `csrf_rejected`；凭据失败：401 `invalid_credentials`；无会话：401 `unauthorized`；无权限：403 `forbidden`。
- 非 JSON：415 `unsupported_media_type`；JSON 语法错误：400 `malformed_json`；字段缺失、未知、类型或约束不符：422 `validation_error`；64 KiB 请求体上限，超出返回 413 `payload_too_large`。
- 账户邮箱重复：409 `email_in_use`（仅 users 邮箱唯一约束映射为该码，其他唯一冲突按 500 记录）。停用或改角色后若不再有启用状态的 admin：409 `last_admin_required`，整笔修改回滚。未知路由：404 `not_found`；方法不允许：405 `method_not_allowed`。
- API 默认每 IP（IPv6 按 /64 前缀合并）每分钟 600 次，登录额外每 IP 每分钟 10 次，可通过环境变量配置；超额返回 429 `rate_limited` 及 `Retry-After: 60`。
- 登录按账户锁定：以规范化邮箱（去首尾空白后转小写）为键，不论账户是否存在都同样计数和锁定，响应不区分两者。15 分钟窗口内连续凭据失败（`invalid_credentials`，含停用账户）达到 `SILVA_LOGIN_LOCKOUT_THRESHOLD` 次（默认 5）后锁定 `SILVA_LOGIN_LOCKOUT_SECONDS` 秒（默认 900）。锁定期内即使密码正确也拒绝，且不校验密码，返回 429 `rate_limited`，`Retry-After` 为剩余锁定秒数（向上取整，至少 1）。成功登录清零；锁定到期后重新计数。Origin、JSON 与字段校验失败及服务端故障不计入。`POST /auth/password` 只共用每 IP 登录额度，不纳入账户锁定。计数保存在单个 API 进程内存中（最多跟踪 10000 个邮箱，满时淘汰最早的未锁定记录），多副本各自计数，重启清空。
- 失败登录审计：`action = "auth.login_failed"`，`actor_user_id = null`，`resource_type = "user"`，账户存在时 `resource_id` 为其 ID，否则为 null；`metadata` 仅含 `{"reason": "invalid_credentials" | "account_disabled" | "locked"}`，不记录尝试的邮箱或密码。`account_disabled` 表示密码正确但账户已停用；`locked` 在同一轮锁定中只于首次被拒时记录一次。审计写入失败时与其他管理操作一致返回 500 `internal_error`，不会放行登录。
- 审计记录 `{id,actor_user_id,action,resource_type,resource_id,ip_address,user_agent,metadata,created_at}`：`ip_address` 为直连 peer IP（不信任 `X-Forwarded-For`，CLI 操作为 null），`user_agent` 截断到 512 字节，`metadata` 只含变更字段名、状态、角色键等非敏感信息。
- User 为 `{id,email,display_name,status,roles,created_at,updated_at}`，不含 password/hash。角色列表返回 `{"data":[{id,key,name,permissions}]}`。
- `/health/live` 验证进程；`/health/ready` 验证数据库与迁移版本；`/openapi.json` 提供运行时契约；每个 HTTP 响应返回服务端生成的 UUIDv7 `X-Request-Id` 和 W3C `X-Trace-Id`；后者为 32 位十六进制链路标识，不是业务实体 ID。有效 `traceparent` 可传入上游追踪上下文。

## UUIDv7 和响应契约

- 实体 ID、外键实体引用、Outbox ID、审计 ID、服务端 `request_id` 均为 UUIDv7，由 `shared::ids` / `Uuid::now_v7()` 统一生成；测试和种子同样遵守。
- API 在路径、查询及 JSON 接受标准连字符 UUID 字符串。格式、版本位或变体位不符统一返回 **422 `validation_error`**，`details` 为 `{"field":"id","code":"invalid_format"}`（字段名随参数而定）。
- OpenAPI 对业务 ID 定义 `type: string`、`format: uuid` 和同时约束版本位、变体位的 `pattern`。
- DB 原生 uuid 类型外，使用 `is_uuid_v7()` 校验实体主键和无法经外键保证的引用。普通/复合外键继承已校验主键约束。
- 外部来源原生 ID、slug、角色键、generation、revision_no、会话/租约 token 不属于实体 ID。
- 响应结构、状态码选择和错误码登记表以 [API 响应结构规范](api-response.md) 为准。概要：成功为 `{"data": ...}`，分页列表另含 `{"meta":{"pagination":{...}}}`；分页默认 `page=1`、`per_page=20`、最大 100，稳定 ID 打破排序同分。失败为 `{"error":{"code","message","details","request_id"}}`，`details` 为字段级错误。
- 公开接口未来只读 `active_generation` 公开投影；尚未上线时返回 503 `site_not_ready`，不得假装空数据。

## 测试锚点

使用真实临时 PostgreSQL 执行全部迁移后测试：

1. 登录成功 Cookie 属性、CSRF token、`auth/me`、登出 204 和登出后 401；DB 只含 token hash。
2. 缺失/错误 Origin、非 JSON 登录、缺失/错误/其他会话 CSRF token 均被拒绝，错误响应统一且包含合法 UUIDv7 request_id。
3. 不存在用户、错误密码、无效/过期会话不获得登录或授权；密码不能明文保存。
4. Author 无法读取账户/角色/审计/系统信息或创建账户；当前角色变更立即影响授权。
5. Admin 创建账户、内置角色分配、停用账户后所有既有会话立即失效；审计可查且不含凭据。
6. API 对 UUIDv4、错误变体、非连字符 UUID 返回 422 `validation_error`，字段错误为 `id`/`invalid_format`；真实 UUIDv7 可进入资源查询。
7. 账户/审计列表默认分页、分页上限、无效分页与无效 JSON，路由不存在、方法不允许等错误保持统一错误结构。
8. 同一邮箱连续登录失败达到阈值后返回 429 与剩余 `Retry-After`，锁定期内正确密码也被拒绝；不存在的账户同样锁定；失败审计按原因记录且不含邮箱、密码；锁定到期或成功登录后计数清零。
9. CI 另验证 UUIDv7 可写入 DB、UUIDv4/错误变体被 DB 拒绝，以及每张业务表/字段拥有非空数据库注释。
