# Identity / RBAC 契约

来源：`docs/01-product.md` §3；`docs/03-backend-technical.md` §3、§5、§9。此契约固定第一里程碑认证与账户管理的验收边界。业务内容、媒体与发布用例在对应里程碑实现。

## 会话和输入边界

- 密码使用 Argon2id；数据库不保存明文密码。
- 认证采用 JWT access token + refresh token。`POST /api/v1/admin/sessions`（登录）与 `POST /api/v1/admin/tokens`（刷新）是无需 access token 的入口，只接受 JSON；其他管理接口只接受 `Authorization: Bearer <access_token>`。不使用 Cookie、CSRF token 或 Origin 校验（Bearer 令牌不会被浏览器自动携带），不启用管理 API 跨域凭证。
- 两种令牌都是 HS256 签名的 JWT，验证时只接受 HS256，拒绝 `none` 与其他算法。声明：`iss = "silva"`、`aud = "silva-admin"`、`sub` 为账户 ID、`sid` 为会话 ID（`sessions.id`）、`jti` 为新 UUIDv7、`iat`、`exp`、`typ = "access" | "refresh"`。校验签名、`exp`、`iss`、`aud` 与 `typ`；refresh token 不能当 access token 使用，反之亦然。令牌不携带角色或权限。
- 服务端会话是撤销依据：每次登录在 `sessions` 中创建一行，`token_hash` 只保存当前有效 refresh token 的 SHA-256 哈希，`expires_at` 为该 refresh token 的过期时间。
- 每个鉴权请求：取唯一的 Bearer 令牌 → 验签与声明 → 按 `sid` 读取会话，会话不存在、已过期或 `user_id` 与 `sub` 不符返回 401 → 读取账户当前状态、角色和权限，停用账户返回 401。因此退出登录、停用、管理员重置密码、修改密码撤销其他会话都会让对应 access token 立即失效，角色变更立即生效。
- `POST /api/v1/admin/tokens` 校验 refresh token 后按 `sid` 锁定会话行：哈希一致则轮换（新 access token + 新 refresh token，更新 `token_hash` 与 `expires_at`，旧 refresh token 立即失效）；哈希不一致视为重放，删除该会话（其全部令牌失效），写入 `auth.refresh_reuse` 审计并返回 401。同一 refresh token 并发提交只有一次成功，落败的一次同样按重放处理。正常刷新不写审计。
- `DELETE /api/v1/admin/sessions/current`（退出登录）删除当前会话，成功 204，无响应体；之后该会话的 access token 与 refresh token 均返回 401。
- `GET /api/v1/admin/users/me` 返回当前登录账户的详情、角色与权限；任何有效会话均可调用。登录与刷新只返回令牌及其过期时间。
- 令牌缺失、格式错误、重复的 Authorization 头、签名或声明无效、会话已撤销返回 401 `unauthorized`；access token 签名有效但已过期返回 401 `token_expired`。所有 401 带 `WWW-Authenticate: Bearer`（过期时为 `Bearer error="invalid_token", ...`）。无权限返回 403 `forbidden`。
- 停用账户立即撤销其全部会话；每次授权使用当前账户状态与当前角色，不能依赖登录时权限快照。
- 认证响应与非公开预览使用 `Cache-Control: no-store`。登录和 API 均需限流，管理操作需审计，不记录密码、access token、refresh token 原文或其他敏感凭据。
- 签名密钥 `SILVA_JWT_SECRET`：生产环境必须设置且至少 32 字节，否则 API 启动失败；开发与测试环境未设置时 API 启动随机生成并告警，重启后已签发的令牌全部失效。CLI 与 Worker 不签发令牌，不要求该变量。密钥不写入日志。

## 角色与归属

| 角色 | 权限边界 |
| --- | --- |
| Visitor | 无需登录，只读已上线公开内容。 |
| Author | 创建文章、读取和编辑自己的文章及修订、上传和引用自己的已校验媒体。 |
| Editor | 管理所有文章及媒体、发布/取消发布、维护文章系列（`series.manage`）、分类标签、个人资料及资源目录、调整 AI 展示、查看相关任务。 |
| Admin | 包含 Editor 权限；额外管理账户、分配内置角色、触发采集、重试发布、查看审计及系统状态。 |

- 即使只有一个管理员，也保留 `users`、`roles`、`permissions`、`user_roles`、`role_permissions`。
- Editor/Admin 可以直接发布当前已保存的文章。
- 服务端逐管理用例执行 RBAC 和资源归属授权；包含嵌套修订读取、上传确认、媒体引用。隐藏前端按钮不算授权。
- 只提供内置角色分配、账户创建、停用与删除，不提供公开注册或自定义角色编辑。

## 第一里程碑管理端点

以下路径相对于 `/api/v1/admin`：

| 方法 | 路径 | 授权及结果 |
| --- | --- | --- |
| POST | `/sessions` | 登录。JSON，无需令牌；创建会话，201，返回 access token、refresh token 及过期时间。 |
| DELETE | `/sessions/current` | 退出登录。Bearer access token；删除当前会话，204。 |
| POST | `/tokens` | 刷新。JSON，无需 Bearer 头；轮换 refresh token，201，返回新令牌对。 |
| GET | `/users/me` | Bearer access token，无需额外权限；当前账户详情、角色、权限。 |
| PUT | `/users/me/password` | Bearer access token；校验当前密码后替换，204；保留当前会话并撤销其他会话。 |
| GET, POST | `/users` | Admin；分页列表（支持关键字、角色、状态筛选）、创建账户（201，`Location` 指向新账户）。 |
| GET | `/users/{id}` | Admin；账户详情。 |
| PATCH | `/users/{id}` | Admin；启用/停用及分配内置角色。 |
| DELETE | `/users/{id}` | Admin；删除其他账户，成功返回 204。禁止自删（403），最后一个启用管理员受并发保护（409），目标不存在或已删除返回 404。 |
| GET | `/roles` | Admin；内置角色。 |
| GET | `/audit-logs` | Admin；审计记录，支持筛选。 |
| GET | `/audit-logs/{id}` | Admin；单条审计记录。 |

以下为主线程根据原文边界冻结的第一里程碑实现约定（原文没有指定这些名称和阈值）：

- 登录输入：`{email,password}`；登录返回 `{"data":{access_token,refresh_token,access_token_expires_at,refresh_token_expires_at}}`，两个 `*_expires_at` 为北京时间（UTC+08:00）RFC 3339 时间，带显式偏移（如 `2026-11-24T16:15:00+08:00`），精确到秒，与令牌 `exp` 声明一致；`GET /users/me` 返回 `{"data":{id,email,display_name,status,roles,created_at,updated_at,last_login_at,permissions}}`。原 `GET /auth/me`、`GET /users/info` 已移除。
- 刷新输入：`{refresh_token}`；返回结构与登录相同：`{"data":{access_token,refresh_token,access_token_expires_at,refresh_token_expires_at}}`。
- 令牌有效期：access token `SILVA_ACCESS_TOKEN_TTL_SECONDS`（默认 900）；refresh token 与会话 `SILVA_REFRESH_TOKEN_TTL_SECONDS`（默认 604800），每次刷新从刷新时刻重新计算。
- 重放审计：`action = "auth.refresh_reuse"`，`actor_user_id = null`，`resource_type = "user"`，`resource_id` 为令牌所属账户，`metadata = {}`，不含任何令牌或哈希。
- 创建账户：`{email,display_name,password,roles:["author"|"editor"|"admin"]}`，201 直接返回 User。
- 修改账户：`{display_name?,status?:"active"|"disabled",roles?:[...],password?}`，200 直接返回 User。`password` 为管理员重置，设置后撤销该账户全部会话。
- 删除账户：清除密码、角色及所有会话，账户从列表、详情和角色计数移除，不能再次启用。原邮箱可用于创建新 ID 的账户。保留原 ID、姓名和邮箱以追溯内容归属和审计，删除事务写入 `users.delete`。
- 修改自己的密码：`{current_password,new_password}`；当前密码错误、新密码不符合 12..1024 字节或与当前密码相同时返回 422 `validation_error`，`details` 分别指向 `current_password`（`incorrect`）或 `new_password`（`out_of_range`、`unchanged`）；与登录共用每 IP 登录限流额度。角色必须是非空、无重复的内置键数组。
- 新账户密码为 12..1024 UTF-8 字节；用户邮箱去首尾空白后转小写，最大 254 字节；显示名去空白后非空，最大 100 字节。
- 凭据失败：401 `invalid_credentials`；令牌缺失、无效、已撤销或 refresh 失败：401 `unauthorized`；access token 过期：401 `token_expired`；无权限：403 `forbidden`。
- 非 JSON：415 `unsupported_media_type`；JSON 语法错误：400 `malformed_json`；字段缺失、未知、类型或约束不符：422 `validation_error`；64 KiB 请求体上限，超出返回 413 `payload_too_large`。
- 账户邮箱重复：409 `email_in_use`（仅 users 邮箱唯一约束映射为该码，其他唯一冲突按 500 记录）。停用或改角色后若不再有启用状态的 admin：409 `last_admin_required`，整笔修改回滚。未知路由：404 `not_found`；方法不允许：405 `method_not_allowed`。
- API 默认每 IP（IPv6 按 /64 前缀合并）每分钟 600 次，登录额外每 IP 每分钟 10 次，可通过环境变量配置；超额返回 429 `rate_limited` 及 `Retry-After: 60`。
- 登录按账户锁定：以规范化邮箱（去首尾空白后转小写）为键，不论账户是否存在都同样计数和锁定，响应不区分两者。15 分钟窗口内连续凭据失败（`invalid_credentials`，含停用账户）达到 `SILVA_LOGIN_LOCKOUT_THRESHOLD` 次（默认 5）后锁定 `SILVA_LOGIN_LOCKOUT_SECONDS` 秒（默认 900）。锁定期内即使密码正确也拒绝，且不校验密码，返回 429 `rate_limited`，`Retry-After` 为剩余锁定秒数（向上取整，至少 1）。成功登录清零；锁定到期后重新计数。JSON 与字段校验失败及服务端故障不计入。`PUT /users/me/password` 只共用每 IP 登录额度，不纳入账户锁定。计数保存在单个 API 进程内存中（最多跟踪 10000 个邮箱，满时淘汰最早的未锁定记录），多副本各自计数，重启清空。
- 失败登录审计：`action = "auth.login_failed"`，`actor_user_id = null`，`resource_type = "user"`，账户存在时 `resource_id` 为其 ID，否则为 null；`metadata` 仅含 `{"reason": "invalid_credentials" | "account_disabled" | "locked"}`，不记录尝试的邮箱或密码。`account_disabled` 表示密码正确但账户已停用；`locked` 在同一轮锁定中只于首次被拒时记录一次。审计写入失败时与其他管理操作一致返回 500 `internal_error`，不会放行登录。
- 审计记录 `{id,actor_user_id,actor,action,resource_type,resource_id,request_id,ip_address,user_agent,metadata,created_at}`：`actor` 为操作账户摘要 `{id,email,display_name}`，系统/CLI 操作为 null；`request_id` 为产生该记录的 HTTP 请求 UUIDv7，与该请求响应头 `X-Request-Id` 一致，CLI 等非 HTTP 操作为 null；`ip_address` 为直连 peer IP（不信任 `X-Forwarded-For`，CLI 操作为 null），`user_agent` 截断到 512 字节，`metadata` 只含变更字段名、状态、角色键等非敏感信息。
- 审计列表 `GET /audit-logs` 可选查询参数以 AND 组合，分页 `total` 反映过滤后结果，排序固定 `created_at DESC, id DESC`；同一参数重复返回 `duplicate`：
  - `from`、`to`：RFC 3339 时间，`from` 含、`to` 不含；格式错误 422 `invalid_format`；同时提供且 `from >= to` 时 422，字段 `to`、码 `out_of_range`。
  - `actor_user_id`、`resource_id`、`request_id`：UUIDv7，非法时 422 `invalid_format`。
  - `action`：精确匹配，或以 `.*` 结尾表示前缀匹配（如 `auth.*` 匹配 `auth.login`）；只允许 `[a-z0-9_.]` 加末尾 `.*`，长度 1..64，否则 422 `invalid_value`。前缀按字面匹配，转义 LIKE 通配符。
  - `resource_type`：精确匹配，`[a-z0-9_]`，长度 1..64，否则 422 `invalid_value`。
- 审计详情 `GET /audit-logs/{id}`：需要 `audit.read`，返回 `{"data": AuditLog}`；不存在 404 `not_found`；非法 UUID 422 `invalid_format`。
- User 为 `{id,email,display_name,status,roles,created_at,updated_at,last_login_at}`，不含 password/hash。`last_login_at` 为最近一次登录成功的 RFC 3339 时间，从未登录时为 `null`，字段总是存在；`/users/me`、账户列表、详情、创建和修改返回的 User 均包含该字段。
- 账户详情：`GET /users/{id}` 需要 `users.manage`，返回 `{"data": User}`；账户不存在返回 404 `not_found`；`id` 不是 UUIDv7 返回 422 `validation_error`（`id`/`invalid_format`）。
- 账户列表筛选：`GET /users` 在 `page`、`per_page` 之外接受可选查询参数，多个条件按 AND 组合，`meta.pagination.total` 为筛选后的数量，排序保持 `created_at DESC, id DESC`。
  - `q`：去首尾空白后按 email 或 display_name 做不区分大小写的子串匹配；LIKE 特殊字符 `%`、`_`、`\` 按字面匹配。去空白后为空视为未提供；去空白后超过 100 字节返回 422（`q`/`out_of_range`）。
  - `role`：`author`、`editor`、`admin` 之一，只返回分配了该角色的账户；其他取值返回 422（`role`/`invalid_value`）。
  - `status`：`active` 或 `disabled`；其他取值返回 422（`status`/`invalid_value`）。
  - 与分页参数相同：同一参数重复返回 422（`duplicate`），未知查询参数忽略，多个字段错误一次返回。
- 角色列表返回 `{"data":[{id,key,name,permissions,user_count}]}`；`user_count` 为分配了该角色的账户数，包含启用和停用账户。
- `/health/live` 验证进程存活；每个 HTTP 响应返回服务端生成的 UUIDv7 `X-Request-Id` 和 W3C `X-Trace-Id`；后者为 32 位十六进制链路标识，不是业务实体 ID。有效 `traceparent` 可传入上游追踪上下文。

## UUIDv7 和响应契约

- 实体 ID、外键实体引用、审计 ID、服务端 `request_id` 均为 UUIDv7，由 `shared::ids` / `Uuid::now_v7()` 统一生成；测试和种子同样遵守。
- API 在路径、查询及 JSON 接受标准连字符 UUID 字符串。格式、版本位或变体位不符统一返回 **422 `validation_error`**，`details` 为 `{"field":"id","code":"invalid_format"}`（字段名随参数而定）。
- OpenAPI 对业务 ID 定义 `type: string`、`format: uuid` 和同时约束版本位、变体位的 `pattern`。
- DB 原生 uuid 类型外，使用 `is_uuid_v7()` 校验实体主键和无法经外键保证的引用。普通/复合外键继承已校验主键约束。
- 外部来源原生 ID、slug、角色键、generation、revision_no、会话/租约 token 不属于实体 ID。
- 响应结构、状态码选择和错误码登记表以 [API 响应结构规范](api-response.md) 为准。概要：成功为 `{"data": ...}`，分页列表另含 `{"meta":{"pagination":{...}}}`；分页默认 `page=1`、`per_page=20`、最大 100，稳定 ID 打破排序同分。失败为 `{"error":{"code","message","details","request_id"}}`，`details` 为字段级错误。
- 公开接口读取已提交的公开内容版本。空站点返回版本 0。

## 测试锚点

使用真实临时 PostgreSQL 执行全部迁移后测试：

1. 登录只返回 access token、refresh token 及两者的过期时间（北京时间 `+08:00`，与 `exp` 一致），Bearer 访问 `users/me` 取得账户详情与权限；令牌声明含 `iss`、`aud`、`sub`、`sid`、`typ`；DB 只含 refresh token 的 SHA-256 哈希，`sessions` 与审计中都没有令牌原文。
2. 缺失、格式错误或重复的 Authorization 头，篡改签名或载荷、其他密钥、错误 `iss`/`aud`、`alg` 为 `none` 或 HS512、refresh token 当 access token、不存在的会话或 `sub` 不符均返回 401 `unauthorized` 并带 `WWW-Authenticate: Bearer`；签名有效的过期 access token 返回 401 `token_expired`；非 JSON 登录被拒绝；错误响应统一，响应头 `X-Request-Id` 为合法 UUIDv7，正文不含 `request_id`。
3. 不存在用户、错误密码、无效/过期会话不获得登录或授权；密码不能明文保存。
4. Author 无法读取账户/角色/审计/系统信息或创建账户；当前角色变更立即影响授权。
5. Admin 创建账户、内置角色分配、停用账户或重置密码后所有既有 access/refresh token 立即失效；修改自己的密码后其他会话令牌失效、当前会话仍可用；审计可查且不含凭据。
6. API 对 UUIDv4、错误变体、非连字符 UUID 返回 422 `validation_error`，字段错误为 `id`/`invalid_format`；真实 UUIDv7 可进入资源查询。
7. 账户/审计列表默认分页、分页上限、无效分页与无效 JSON，路由不存在、方法不允许等错误保持统一错误结构。
8. 账户 `last_login_at` 登录前为 null、登录后为时间；账户详情 200/404/422；列表按 `q`、`role`、`status` 单独及组合筛选，`total` 反映筛选结果，LIKE 特殊字符按字面匹配，非法取值返回字段错误；角色 `user_count` 包含停用账户。
9. 审计记录的 `request_id` 与写操作响应头 `X-Request-Id` 一致，CLI 初始化管理员的记录 `request_id`、`actor` 为 null；各筛选条件、非法筛选值、详情 200/404/422 及 Author 无权访问均有覆盖。
10. 同一邮箱连续登录失败达到阈值后返回 429 与剩余 `Retry-After`，锁定期内正确密码也被拒绝；不存在的账户同样锁定；失败审计按原因记录且不含邮箱、密码；锁定到期或成功登录后计数清零。
11. CI 另验证 UUIDv7 可写入 DB、UUIDv4/错误变体被 DB 拒绝，以及每张业务表/字段拥有非空数据库注释。
12. 刷新轮换后新令牌可用；重放旧 refresh token 返回 401，整个会话（含新令牌）失效并写入 `auth.refresh_reuse` 审计；同一 refresh token 并发提交只有一次成功；退出登录后 access 与 refresh token 均返回 401；服务端会话过期后两种令牌均失效。
