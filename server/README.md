# Silva 服务端

Rust + Axum + Tokio + SQLx + PostgreSQL 的模块化单体。API、Worker 和运维 CLI 共用领域与用例，分别运行。

当前交付后端技术方案的前四个里程碑：Workspace、环境配置、数据库迁移与检查、健康检查、Argon2id 登录、JWT access token / refresh token 认证（服务端会话可撤销）、内置角色和账户管理；经服务端校验并去重后上传到 Cloudflare R2 或阿里云 OSS 的图片媒体；文章的草稿保存、分类标签、发布与取消发布；以及事务内发布和撤回。Astro SSR 与 Public API 共用数据库中的已发布内容。Editor/Admin 可整体保存个人资料，由同一发布流程更新 About 页面与公开 API。GitHub 采集排在后续里程碑。Worker 执行会话维护与过期媒体清理。

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
| `SILVA_STORAGE_REGION` | R2 默认 `auto`；OSS 必填，为不含 `oss-` 前缀的地域 ID，如 `cn-hangzhou`（请求使用 OSS V4 签名） |
| `SILVA_STORAGE_ROOT` | `/`；bucket 内的根前缀，多个环境共用一个 bucket 时用于隔离，如 `/silva/production/`；路径段只允许 `A-Za-z0-9._-` |
| `SILVA_STORAGE_ACCESS_KEY_ID`、`SILVA_STORAGE_ACCESS_KEY_SECRET` | 设置服务商时必填；只读取显式配置，不读取云主机元数据或默认凭据文件 |
| `SILVA_SITE_URL` | Compose 传给 Astro 的公开 origin，默认 `http://localhost:8088`。用于 canonical、RSS 与 sitemap |
| `SILVA_MEDIA_UPLOAD_TTL_SECONDS` | `900`，正整数秒，最大 `86400`；查看链接有效期，也用于未完成上传的清理期限 |
| `RUST_LOG` | `.env.example` 使用 `info,silva_api=debug` |

初始化 CLI 额外读取 `SILVA_ADMIN_EMAIL`、`SILVA_ADMIN_NAME` 和 `SILVA_ADMIN_PASSWORD`。集成测试单独读取 `SILVA_TEST_DATABASE_URL`，不复用生产连接。

API 与登录限流使用按 IP（IPv6 合并到 /64）的 60 秒窗口。直接部署默认只信任 socket peer。Compose 将 API 放在内部网络，设置 `SILVA_TRUST_PROXY_IP_HEADER=true`，由网关用实际连接地址覆盖 `X-Real-IP`。该模式只适用于所有直连方均受信的私有监听地址。Astro 通过内部快照路径校验页面版本；网关阻止外部访问该路径。

登录另按规范化邮箱执行连续失败锁定（不论账户是否存在），锁定期内返回 429 `rate_limited`，`Retry-After` 为剩余锁定秒数。该计数同样保存在单个 API 进程内存中：多副本部署时各副本独立计数，重启后清空；需要跨副本一致时应改为共享存储。

## 媒体存储

每个 API 进程最多同时处理 4 次上传或确认。服务端在 10 MiB 上限内缓冲单张图片，完整解码后按文件字节的 SHA-256 去重。同一作者重复上传会取得原媒体 ID 和原替代文本。不同作者各自拥有媒体条目，共享私有 bucket 中的一份文件。成功时返回可直接打开的短期只读链接。后台也可使用媒体 ID 授权读取原图。[API 接入指南](docs/api/README.md#媒体上传)

R2 使用限制在媒体 bucket 的对象读写令牌，OSS 使用限制在对应 bucket 或根前缀的读写、删除权限。关闭公共读写，OSS 保持版本控制关闭以支持禁止覆盖。上传由服务端发起。升级前已签发的旧版直传地址到期前仍应保留原有的 bucket CORS 和 `uploads/` 临时对象生命周期规则。

使用以下命令验证流式写入、禁止覆盖、只读链接和删除。测试对象使用唯一的 `smoke/` 键。

```sh
export SILVA_STORAGE_PROVIDER=r2 SILVA_STORAGE_ENDPOINT=https://<account_id>.r2.cloudflarestorage.com \
  SILVA_STORAGE_BUCKET=<bucket> SILVA_STORAGE_ROOT=/silva/development/
read -r SILVA_STORAGE_ACCESS_KEY_ID
read -r -s SILVA_STORAGE_ACCESS_KEY_SECRET
export SILVA_STORAGE_ACCESS_KEY_ID SILVA_STORAGE_ACCESS_KEY_SECRET
cargo test -p silva-infrastructure --test object_storage_live -- --ignored
```

## 文章与系列的同步上线

Editor/Admin 在编辑器点击发布时，编辑器先保存当时的内容，再提交该修订 ID 和发布版本。数据库在同一事务内更新独立的公开副本、版本与审计记录。成功返回 200 后文章可读。后续编辑只更新草稿。撤回提交后公开详情与不再被公开内容引用的图片立即返回 404。

Astro 在请求时从 `GET /api/v1/public/site-snapshot` 取得同一版本的公开内容，生成首页、文章、系列、归档、RSS 与 sitemap。首次安装的空站点为版本 0。系列中过滤掉草稿文章、空篇章和空系列。

在 `server/` 执行 `make up` 可一起启动网关、Astro、API 与维护 Worker。公开站与公共 API 使用 `http://localhost:8088`；Admin 的同源代理和 Vite 的 `SILVA_API_TARGET` 指向 `http://127.0.0.1:8089`。运维网络通过 `http://127.0.0.1:8090` 读取健康状态与指标。部署时分别转发这三个监听端口。Admin 开发代理还需将公开 API 指向 8088、健康检查与指标指向 8090，配置示例见 [Admin 本地开发](../frontend/apps/admin/README.md)。批量导入工具通过 `SILVA_API_URL=http://127.0.0.1:8089` 访问管理入口。网站镜像只在部署代码时编译。对象存储用于媒体；发布纯文本文章只依赖数据库和公开站服务。

### 升级到同步内容版本

迁移至 `20261128000000` 后，公开修订和本次发布时间统一从 `published_posts` 读取。迁移清除旧发布任务、事件队列及仅属于任务的审计记录，保留当前公开版本、文章草稿、公开正文和内容操作审计。应用迁移时停止旧 API 写入，配套升级 API、后台及调用管理接口的客户端。新客户端读取 `published_revision_id`，上线和撤回返回 `generation` 与 `publication_version`。

### 从静态构建版本升级

先暂停旧系统的发布写入和构建 Worker，检查 `SELECT active_generation, desired_generation FROM site_state`。两者相等（含都为 NULL）时可执行迁移并一起切换 API 和公开站入口。已有线上文章保留发布副本，未提交的草稿继续留在编辑器。

如果两个版本之间仅有一次失败的专栏发布，该专栏没有已发布成员，迁移会核对发布审计、线上文章和可见专栏的映射。确认公开内容一致后沿用原线上版本，保留专栏的公开设置。以后发布首篇成员文章时，该专栏会一同可见。

其他版本差异仍会中止迁移。保持旧站服务，从后台核对差异，作者确认需要上线时再用旧版发布。新旧 API 和 Worker 不应同时写入同一个数据库。

## 检查与测试

```sh
make api-docs-check
make check
make check-schema
SILVA_TEST_DATABASE_URL=postgres://silva:silva@127.0.0.1:55439/silva make test-db
make worker-once
```

`make test-db` 包含身份、媒体、文章发布与系列编排集成测试。文章与发布测试断言提交后公开接口、系列、媒体及快照的可见性。

`make check` 执行格式、Clippy（警告视为错误）、Workspace 构建和普通测试，其中包括 Rust OpenAPI 契约、Schema 及示例校验。`make api-docs-check` 可单独运行契约测试。`make test-db` 显式执行数据库集成测试，并按契约校验真实接口的状态、媒体类型、响应头和 JSON Schema：连接角色需有 `CREATEDB`，测试创建独立临时数据库并在结束后清理。缺少测试 URL 或数据库不可用会失败，不把数据库测试静默当作成功。

`check-schema` 在迁移后的真实数据库检查业务表/字段非空注释、UUIDv7 约束及媒体状态约束；测试覆盖 UUIDv7 写入、v4/错误变体拒绝和 API 非 v7 ID 拒绝，登录、JWT 校验与刷新轮换、RBAC、账户停用和会话撤销，以及媒体申请、确认、拒绝、过期、归属、引用保护和清理（对象存储用内存实现，图片解码为真实实现）。GitHub Actions 使用临时 PostgreSQL 执行相同门禁。

所有业务实体、事件、审计和 request ID 都通过 `silva_shared::ids` 生成 UUIDv7。新增或变更数据库字段时，在同一次迁移维护 `COMMENT ON TABLE/COLUMN`；已执行的迁移不修改，追加新迁移。

旧开发库执行 `make migrate` 后会删除已取消的目录功能对应的四张表，以及项目展览与工具导航对应的四张表（`projects`、`project_technologies`、`project_links`、`tools`）及其数据，再运行 `make check-schema` 验证；历史迁移保留原校验和。

## 容器运行

基础配置使用 `docker-compose.yaml`，观测栈通过 `docker-compose.observability.yaml` 覆盖配置启用，具体操作见 [观测栈说明](observability/README.md)。以下命令均在 `server/` 目录执行。

```sh
make up
```

API、CLI 和 Worker 使用 Rust 镜像；网站使用 `website` 目标构建的 Node SSR 镜像。网关的公开、管理、运维端口分别为 8088、8089、8090，宿主机均绑定回环地址。Compose 使用内部 API 地址访问站点快照。应用栈使用 `app` profile，`make db-up` 可单独启动数据库。

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

`make down` 保留数据库卷。Compose 中的明文账户和本地端口配置仅供开发；生产分别部署 API/Worker，通过环境 Secret 管理凭据，后台入口代理同源 API，公共入口只暴露公共路由。

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
