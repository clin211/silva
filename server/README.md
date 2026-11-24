# Silva 服务端

Rust + Axum + Tokio + SQLx + PostgreSQL 的模块化单体。API、Worker 和运维 CLI 共用领域与用例，分别运行。

当前交付后端技术方案的第一里程碑，以及第二里程碑中的媒体上传：Workspace、环境配置、数据库迁移与检查、健康检查、Argon2id 登录、JWT access token / refresh token 认证（服务端会话可撤销）、内置角色和账户管理，以及经 Cloudflare R2 或阿里云 OSS 私有 bucket 直传、服务端校验的图片媒体。数据库已预建后续领域的 schema；CMS、站点构建与切换、资源目录和 GitHub 采集在后续里程碑实现。Worker 当前执行会话维护和过期媒体清理，不消费尚未实现的发布或采集任务。

## 本地启动

需要 Rust `1.100.0`（`rust-toolchain.toml` 固定）、Docker Compose v2 和 `make`。从本目录运行：

```sh
cp .env.example .env
make db-up
make migrate
make check-schema
```

PostgreSQL 绑定 `127.0.0.1:55439`，开发库、用户名、密码均为 `silva`。该角色可以创建数据库，供隔离集成测试使用。`.env` 仅供本机开发，生产通过运行环境注入 Secret。

初始化第一个管理员（密码不作为命令参数传递）：

```sh
export SILVA_ADMIN_EMAIL=admin@example.com
export SILVA_ADMIN_NAME=Administrator
printf '管理员密码: '
read -r -s SILVA_ADMIN_PASSWORD
printf '\n'
export SILVA_ADMIN_PASSWORD
cargo run --locked -p silva-cli -- seed-admin
unset SILVA_ADMIN_PASSWORD
```

也可用 `--email` 和 `--display-name` 指定邮箱与名称。重复执行初始化不会覆盖已有账户密码或权限。不要在生产使用演示密码。

创建其他角色的账户使用 `create-user`，`--role` 可重复指定（`author`、`editor`、`admin`），密码读取 `SILVA_USER_PASSWORD`：

```sh
printf '账户密码: '
read -r -s SILVA_USER_PASSWORD
printf '\n'
export SILVA_USER_PASSWORD
cargo run --locked -p silva-cli -- create-user --email author@example.com --display-name 作者 --role author
unset SILVA_USER_PASSWORD
```

本地开发要快速得到一个随机邮箱和密码的账户，运行 `scripts/create-user.sh [--role author|editor|admin]...`（默认 admin），凭据会打印到终端；本机没有 cargo 时脚本自动改用 Docker 中的 Rust 工具链。

在两个终端分别启动：

```sh
make api
```

```sh
make worker
```

API 默认监听 `127.0.0.1:8088`。管理接口使用 `Authorization: Bearer <access_token>` 认证，生产环境通过 HTTPS 入口访问。

```sh
curl --fail http://127.0.0.1:8088/health/live
curl --fail http://127.0.0.1:8088/metrics
```

`/health/live` 检查进程存活，`/metrics` 提供 Prometheus 运维指标；生产入口应限制 metrics 的外部访问。业务路径及 DTO 以 [OpenAPI 文件](apps/api/openapi.json) 为准，未实现的业务端点不在契约中承诺。

## API 文档

API 文档交付为 [OpenAPI 3.1 JSON](apps/api/openapi.json)，导入 Apifox 进行在线预览和调试。[中文接入指南](docs/api/README.md) 说明导入、登录与令牌刷新、分页、权限和错误处理。

## 可观测性

运行 `make observability-up` 启动完整本地观测栈，Grafana 位于 http://localhost:3001，提供 API、数据库、Worker、发布与采集四张看板。日志与链路经 OpenTelemetry Collector 发送到 Loki / Tempo；Prometheus 指标与 Alertmanager 规则自动配置。本地默认账户与运维说明见 [可观测性文档](observability/README.md)。

`make observability-check` 校验配置及告警行为，`make observability-smoke` 验证真实日志、链路与抓取状态。数据库故障时 `/metrics` 仍然可用，业务队列旧值通过采集状态和最后成功时间识别。每个 API 响应同时返回 UUIDv7 `X-Request-Id` 和 W3C `X-Trace-Id`。

## 登录与令牌

登录只接受 JSON，返回 access token 与 refresh token。示例需要 `jq`，使用交互输入密码，经标准输入传给 curl，避免写入命令历史：

```sh
export SILVA_ADMIN_EMAIL=admin@example.com
printf '登录密码: '
read -r -s SILVA_LOGIN_PASSWORD
printf '\n'
export SILVA_LOGIN_PASSWORD
SILVA_LOGIN_JSON=$(jq --null-input --compact-output '{email: env.SILVA_ADMIN_EMAIL, password: env.SILVA_LOGIN_PASSWORD}' \
  | curl --fail-with-body --silent --show-error http://127.0.0.1:8088/api/v1/admin/sessions \
      -H 'Content-Type: application/json' \
      --data-binary @-)
unset SILVA_LOGIN_PASSWORD
SILVA_ACCESS_TOKEN=$(jq -er '.data.access_token' <<<"$SILVA_LOGIN_JSON")
SILVA_REFRESH_TOKEN=$(jq -er '.data.refresh_token' <<<"$SILVA_LOGIN_JSON")
unset SILVA_LOGIN_JSON
```

成功响应只含两个令牌及其过期时间：`{"data": {"access_token", "refresh_token", "access_token_expires_at", "refresh_token_expires_at"}}`，两个 `*_expires_at` 为北京时间（UTC+08:00）RFC 3339 时间，带显式偏移（如 `2026-11-24T16:15:00+08:00`），精确到秒，与令牌 `exp` 声明一致（响应结构见 [API 响应结构规范](docs/contracts/api-response.md)）。之后的管理接口携带 access token；当前账户详情、角色与权限通过 `GET /users/me` 读取：

```sh
curl --fail-with-body http://127.0.0.1:8088/api/v1/admin/users/me \
  -H "Authorization: Bearer $SILVA_ACCESS_TOKEN"
```

access token 默认 15 分钟过期，过期返回 401 `token_expired`。用 refresh token 换取新的令牌对；refresh token 每次使用都会轮换，旧值立即失效，必须保存新值：

```sh
SILVA_REFRESH_JSON=$(jq --null-input --compact-output --arg token "$SILVA_REFRESH_TOKEN" '{refresh_token: $token}' \
  | curl --fail-with-body --silent --show-error http://127.0.0.1:8088/api/v1/admin/tokens \
      -H 'Content-Type: application/json' \
      --data-binary @-)
SILVA_ACCESS_TOKEN=$(jq -er '.data.access_token' <<<"$SILVA_REFRESH_JSON")
SILVA_REFRESH_TOKEN=$(jq -er '.data.refresh_token' <<<"$SILVA_REFRESH_JSON")
unset SILVA_REFRESH_JSON
```

退出登录删除当前会话，该会话的 access token 和 refresh token 随即失效：

```sh
curl --fail-with-body -X DELETE http://127.0.0.1:8088/api/v1/admin/sessions/current \
  -H "Authorization: Bearer $SILVA_ACCESS_TOKEN"
unset SILVA_ACCESS_TOKEN SILVA_REFRESH_TOKEN
```

两种令牌都是 HS256 签名的 JWT，只携带账户 ID 与会话 ID；服务端每次请求都按会话读取账户当前状态、角色和权限，数据库只保存当前 refresh token 的 SHA-256 哈希。已轮换的 refresh token 再次提交视为重放：整个会话被撤销并写入 `auth.refresh_reuse` 审计。登录与刷新之外的管理接口执行令牌和 RBAC 检查，账户停用或管理员重置密码会撤销该账户全部会话；`PUT /api/v1/admin/users/me/password` 修改自己的密码，保留当前会话并撤销其他会话。内置 Author、Editor、Admin 权限来自产品文档 §3；账户管理、内置角色读取与审计属于 Admin 权限。

## 配置

进程读取工作目录下的 `.env`，已设置的环境变量优先。配置非法时启动失败，数据库连接信息不会写入日志。

| 变量 | 默认值 / 约束 |
| --- | --- |
| `DATABASE_URL` | 必填 PostgreSQL URL；本机示例在 `.env.example` |
| `SILVA_BIND` | `127.0.0.1:8088` |
| `SILVA_ENV` | `development`；可选 `test`、`production` |
| `SILVA_DB_MAX_CONNECTIONS` | `10`，正整数 |
| `SILVA_JWT_SECRET` | JWT（HS256）签名密钥，至少 32 字节；生产环境必填，否则 API 启动失败。开发/测试未设置时 API 随机生成并告警，重启后令牌失效。CLI、Worker 不需要 |
| `SILVA_ACCESS_TOKEN_TTL_SECONDS` | `900`，正整数秒；access token 有效期 |
| `SILVA_REFRESH_TOKEN_TTL_SECONDS` | `604800`，正整数秒；refresh token 与会话有效期，每次刷新重新计算 |
| `SILVA_WORKER_INTERVAL_SECONDS` | `60`，正整数秒 |
| `SILVA_API_RATE_PER_MINUTE` | `600`，正整数 |
| `SILVA_LOGIN_RATE_PER_MINUTE` | `10`，正整数 |
| `SILVA_LOGIN_LOCKOUT_THRESHOLD` | `5`，正整数；同一邮箱 15 分钟内连续登录失败多少次后锁定 |
| `SILVA_LOGIN_LOCKOUT_SECONDS` | `900`，正整数秒；账户登录锁定时长 |
| `SILVA_STORAGE_PROVIDER` | 空；`r2` 或 `oss`。生产环境必填，否则 API 启动失败；开发环境留空时媒体上传、确认、读取内容和删除返回 503 `storage_unavailable` |
| `SILVA_STORAGE_ENDPOINT` | 设置服务商时必填。R2 为 `https://<account_id>.r2.cloudflarestorage.com`，OSS 为地域地址如 `https://oss-cn-hangzhou.aliyuncs.com`（不含 bucket）；生产只接受 HTTPS |
| `SILVA_STORAGE_BUCKET` | 设置服务商时必填，私有 bucket 名称，只含 `a-z`、`0-9`、`-`、`.` |
| `SILVA_STORAGE_REGION` | R2 默认 `auto`；OSS 必填，为不含 `oss-` 前缀的地域 ID，如 `cn-hangzhou`（直传地址使用 OSS V4 签名） |
| `SILVA_STORAGE_ROOT` | `/`；bucket 内的根前缀，多个环境共用一个 bucket 时用于隔离，如 `/silva/production/`；路径段只允许 `A-Za-z0-9._-` |
| `SILVA_STORAGE_ACCESS_KEY_ID`、`SILVA_STORAGE_ACCESS_KEY_SECRET` | 设置服务商时必填；只读取显式配置，不读取云主机元数据或默认凭据文件 |
| `SILVA_MEDIA_UPLOAD_TTL_SECONDS` | `900`，正整数秒，最大 `86400`；直传地址与上传确认的有效期，需短于 `uploads/` 的生命周期规则 |
| `RUST_LOG` | `.env.example` 使用 `info,silva_api=debug` |

初始化 CLI 额外读取 `SILVA_ADMIN_EMAIL`、`SILVA_ADMIN_NAME` 和 `SILVA_ADMIN_PASSWORD`。集成测试单独读取 `SILVA_TEST_DATABASE_URL`，不复用生产连接。

API 和登录限流目前按直连 peer IP（IPv6 按 /64 前缀合并）使用进程内 60 秒固定窗口，API 与登录额度同时生效；不信任客户端提供的 `X-Forwarded-For`。通过同源反向代理部署时，后端看到的是代理 IP，经过该代理的请求会共享额度。生产入口应按可信的真实客户端 IP 限流，并在多个后端副本之间统一执行限制；后端阈值需结合代理汇总流量配置。重启进程会清空本机限流窗口。

登录另按规范化邮箱执行连续失败锁定（不论账户是否存在），锁定期内返回 429 `rate_limited`，`Retry-After` 为剩余锁定秒数。该计数同样保存在单个 API 进程内存中：多副本部署时各副本独立计数，重启后清空；需要跨副本一致时应改为共享存储。

## 媒体存储

媒体只存放在私有 bucket。浏览器用 API 签发的短时预签名地址把文件直传到临时键 `uploads/{id}`，文件不经过 API。签名覆盖 `Content-Type` 和 `Content-Length`：R2 使用 SigV4，OSS 使用 V4 签名，上传的字节数必须与申请时声明的 `size_bytes` 完全一致，否则存储服务拒绝该请求，因此预签名地址不能用来上传超大文件。确认时服务端读取文件，按实际字节识别 JPEG、PNG、WebP 并完整解码，通过后写入不可覆盖的正式键 `media/{id}`，并删除临时对象。后台通过 `GET /api/v1/admin/media/{id}/content` 授权读取，bucket 本身不对外公开。流程与接口见 [API 接入指南](docs/api/README.md#媒体上传)。

R2 与 OSS 都需要按以下要求配置 bucket：

1. **私有访问。** 关闭公共读写，R2 不绑定公开域名或 `r2.dev` 访问。
2. **访问密钥最小权限。** R2 创建只作用于该 bucket 的「对象读和写」API 令牌；OSS 使用 RAM 子账号，只授予该 bucket（或 `SILVA_STORAGE_ROOT` 前缀）的 `oss:PutObject`、`oss:GetObject`、`oss:DeleteObject`。
3. **CORS。** 只允许后台 origin 使用 `PUT` 方法和 `content-type` 请求头，例如 R2：

   ```json
   [
     {
       "AllowedOrigins": ["https://admin.example.com"],
       "AllowedMethods": ["PUT"],
       "AllowedHeaders": ["content-type"],
       "MaxAgeSeconds": 3600
     }
   ]
   ```

   OSS 在「数据安全 → 跨域设置」添加同样的来源、方法 `PUT` 和允许 Headers `content-type`。本地开发再加入本机后台地址，如 `http://localhost:5173`。
4. **生命周期规则（必须配置）。** 为 `uploads/` 前缀（若设置了 `SILVA_STORAGE_ROOT`，前缀要包含根路径）添加 1 天后删除对象的规则。Worker 只清理仍有待确认或已拒绝记录对应的临时对象：过期 1 小时以上的未确认上传、被拒绝满 7 天的上传，以及中断的确认可能留下的正式对象。以下临时对象只能靠该规则删除：确认成功后在直传地址有效期内被再次上传的文件、直传后从未调用 API 的文件，以及 Worker 删除失败的文件。不要对 `media/` 设置过期规则。
5. **不开启版本控制（OSS）。** OSS 在开启或暂停版本控制的 bucket 上会忽略禁止覆盖写入的请求头，正式对象将不再受保护。R2 使用 `If-None-Match` 条件写入，不受此影响。

配置完成后用真实 bucket 运行冒烟测试。测试只用预签名地址直传，确认大小与签名不符的上传被拒绝，并验证条件写入、读取和删除，测试对象写在根前缀下的 `smoke/`，结束时删除：

```sh
export SILVA_STORAGE_PROVIDER=r2 SILVA_STORAGE_ENDPOINT=https://<account_id>.r2.cloudflarestorage.com \
  SILVA_STORAGE_BUCKET=<bucket> SILVA_STORAGE_ROOT=/silva/development/
read -r SILVA_STORAGE_ACCESS_KEY_ID
read -r -s SILVA_STORAGE_ACCESS_KEY_SECRET
export SILVA_STORAGE_ACCESS_KEY_ID SILVA_STORAGE_ACCESS_KEY_SECRET
cargo test -p silva-infrastructure --test object_storage_live -- --ignored
```

该测试不经过浏览器，因此不检查 CORS；CORS 需在后台页面实际上传一次确认。

## 检查与测试

```sh
make api-docs-check
make check
make check-schema
SILVA_TEST_DATABASE_URL=postgres://silva:silva@127.0.0.1:55439/silva make test-db
make worker-once
```

`make check` 执行格式、Clippy（警告视为错误）、Workspace 构建和普通测试，其中包括 Rust OpenAPI 契约、Schema 及示例校验。`make api-docs-check` 可单独运行契约测试。`make test-db` 显式执行数据库集成测试，并按契约校验真实接口的状态、媒体类型、响应头和 JSON Schema：连接角色需有 `CREATEDB`，测试创建独立临时数据库并在结束后清理。缺少测试 URL 或数据库不可用会失败，不把数据库测试静默当作成功。

`check-schema` 在迁移后的真实数据库检查业务表/字段非空注释、UUIDv7 约束及媒体状态约束；测试覆盖 UUIDv7 写入、v4/错误变体拒绝和 API 非 v7 ID 拒绝，登录、JWT 校验与刷新轮换、RBAC、账户停用和会话撤销，以及媒体申请、确认、拒绝、过期、归属、引用保护和清理（对象存储用内存实现，图片解码为真实实现）。GitHub Actions 使用临时 PostgreSQL 执行相同门禁。

所有业务实体、事件、审计和 request ID 都通过 `silva_shared::ids` 生成 UUIDv7。新增或变更数据库字段时，在同一次迁移维护 `COMMENT ON TABLE/COLUMN`；已执行的迁移不修改，追加新迁移。

数据库当前保留 27 张业务表。旧开发库执行 `make migrate` 后会删除已取消的目录功能对应的四张表，以及项目展览与工具导航对应的四张表（`projects`、`project_technologies`、`project_links`、`tools`）及其数据，再运行 `make check-schema` 验证；历史迁移保留原校验和。

## 容器运行

基础配置使用 `docker-compose.yaml`，观测栈通过 `docker-compose.observability.yaml` 覆盖配置启用，具体操作见 [观测栈说明](observability/README.md)。以下命令均在 `server/` 目录执行。

```sh
make up
```

这会构建同一镜像的三个 binary，等待 PostgreSQL 健康和一次性迁移成功后启动 API、Worker。镜像运行用户为非 root，支持 SIGTERM 优雅退出。完整栈使用 Compose 的 `app` profile；`make db-up` 仅启动数据库，适合在宿主机开发。

在容器栈初始化管理员时，先按前文交互设置三个管理员环境变量，再运行：

```sh
docker compose --profile app run --rm \
  -e SILVA_ADMIN_EMAIL -e SILVA_ADMIN_NAME -e SILVA_ADMIN_PASSWORD \
  migrate silva-cli seed-admin
unset SILVA_ADMIN_PASSWORD
```

```sh
docker compose logs -f api worker
make down
```

`make down` 保留数据库卷。Compose 中的明文账户和本地端口配置仅供开发；生产分别部署 API/Worker，通过环境 Secret 管理凭据，后台入口代理同源 API，公共入口只暴露公共路由。发布里程碑仍需实现基于 `site_state.active_generation` 的统一读取和产物切换后才能上线公开站点。

## 目录

```text
apps/api                 HTTP、认证边界、DTO、OpenAPI
apps/worker              独立维护进程
apps/cli                 迁移、schema 检查、管理员初始化
crates/domain            纯领域规则
crates/application       用例与 ports
crates/infrastructure    SQLx、密码哈希等适配器
crates/shared            配置、错误、UUIDv7、分页、日志
xtask                    Rust 观测工具：本地告警接收、冒烟校验、故障演练
migrations               SQLx 数据库版本
docs/contracts           本次实现契约
docs/api                 API 中文接入指南与文档维护约定
```

依赖方向遵循后端方案 §2.1：domain 不依赖 Axum/SQLx/infrastructure，用例不放进 HTTP handler。
