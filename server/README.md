# Silva 服务端

Rust + Axum + Tokio + SQLx + PostgreSQL 的模块化单体。API、Worker 和运维 CLI 共用领域与用例，分别运行。

当前交付后端技术方案的第一里程碑：Workspace、环境配置、数据库迁移与检查、健康检查、Argon2id 登录、服务端会话、CSRF、内置角色和账户管理。数据库已预建后续领域的 schema；媒体、CMS、站点构建与切换、资源目录和 GitHub 采集在后续里程碑实现。Worker 当前执行会话维护，不消费尚未实现的发布或采集任务。

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

在两个终端分别启动：

```sh
make api
```

```sh
make worker
```

API 默认监听 `127.0.0.1:8088`。浏览器后台 Origin 默认是 `http://localhost:5173`，浏览器应通过后台同源 `/api/v1/admin` 代理访问 API。

```sh
curl --fail http://127.0.0.1:8088/health/live
curl --fail http://127.0.0.1:8088/health/ready
curl --fail http://127.0.0.1:8088/openapi.json
```

`/health/live` 检查进程，`/health/ready` 检查数据库就绪；`/metrics` 提供 Prometheus 运维指标。生产入口应限制健康详情、OpenAPI 和 metrics 的外部访问。业务路径及 DTO 以 `/openapi.json` 为准，未实现的业务端点不在契约中承诺。

## API 文档

API 文档交付为 [OpenAPI 3.1 JSON](apps/api/openapi.json)，导入 Apifox 进行在线预览和调试。运行时 `/openapi.json` 返回同一份契约。[中文接入指南](docs/api/README.md) 说明导入、登录、Cookie、CSRF、分页、权限和错误处理。

## 可观测性

运行 `make observability-up` 启动完整本地观测栈，Grafana 位于 http://localhost:3001，提供 API、数据库、Worker、发布与采集四张看板。日志与链路经 OpenTelemetry Collector 发送到 Loki / Tempo；Prometheus 指标与 Alertmanager 规则自动配置。本地默认账户与运维说明见 [可观测性文档](observability/README.md)。

`make observability-check` 校验配置及告警行为，`make observability-smoke` 验证真实日志、链路与抓取状态。数据库故障时 `/metrics` 仍然可用，业务队列旧值通过采集状态和最后成功时间识别。每个 API 响应同时返回 UUIDv7 `X-Request-Id` 和 W3C `X-Trace-Id`。

## 登录与会话

登录只接受 JSON 和精确匹配 `SILVA_ADMIN_ORIGIN` 的 `Origin`。示例需要 `jq`，使用交互输入密码，经标准输入传给 curl，避免写入命令历史：

```sh
export SILVA_ADMIN_EMAIL=admin@example.com
printf '登录密码: '
read -r -s SILVA_LOGIN_PASSWORD
printf '\n'
export SILVA_LOGIN_PASSWORD
jq --null-input --compact-output '{email: env.SILVA_ADMIN_EMAIL, password: env.SILVA_LOGIN_PASSWORD}' \
  | curl --fail-with-body -i http://127.0.0.1:8088/api/v1/admin/auth/login \
      -H 'Content-Type: application/json' \
      -H 'Origin: http://localhost:5173' \
      --data-binary @-
unset SILVA_LOGIN_PASSWORD
```

成功响应的 `Set-Cookie` 提供会话，JSON 顶层提供 `csrf_token`。会话 Cookie 始终为 HttpOnly、Secure、SameSite=Lax、Path=/、host-only；数据库只保存令牌哈希。使用 HTTPS 代理进行浏览器联调。HTTP curl 冒烟若客户端不回传 Secure Cookie，可手动把响应 Cookie 的名称和值填入后续 `Cookie` 头：

```sh
curl --fail-with-body http://127.0.0.1:8088/api/v1/admin/auth/me \
  -H 'Cookie: <Set-Cookie 中的名称>=<令牌>'
```

管理写操作还必须携带精确 Origin 和对应会话的 `X-CSRF-Token`；`auth/me` 可重新取得 CSRF token。登录之外的管理接口执行会话和 RBAC 检查，账户停用会撤销会话。内置 Author、Editor、Admin 权限来自产品文档 §3；账户管理、内置角色读取与审计属于 Admin 权限。

## 配置

进程读取工作目录下的 `.env`，已设置的环境变量优先。配置非法时启动失败，数据库连接信息不会写入日志。

| 变量 | 默认值 / 约束 |
| --- | --- |
| `DATABASE_URL` | 必填 PostgreSQL URL；本机示例在 `.env.example` |
| `SILVA_BIND` | `127.0.0.1:8088` |
| `SILVA_ADMIN_ORIGIN` | `http://localhost:5173`；精确 Origin，无尾部 `/` |
| `SILVA_ENV` | `development`；可选 `test`、`production`，生产 Origin 必须 HTTPS |
| `SILVA_DB_MAX_CONNECTIONS` | `10`，正整数 |
| `SILVA_SESSION_TTL_SECONDS` | `43200`，正整数秒 |
| `SILVA_WORKER_INTERVAL_SECONDS` | `60`，正整数秒 |
| `SILVA_API_RATE_PER_MINUTE` | `600`，正整数 |
| `SILVA_LOGIN_RATE_PER_MINUTE` | `10`，正整数 |
| `RUST_LOG` | `.env.example` 使用 `info,silva_api=debug` |

初始化 CLI 额外读取 `SILVA_ADMIN_EMAIL`、`SILVA_ADMIN_NAME` 和 `SILVA_ADMIN_PASSWORD`。集成测试单独读取 `SILVA_TEST_DATABASE_URL`，不复用生产连接。

API 和登录限流目前按直连 peer IP 使用进程内 60 秒固定窗口，API 与登录额度同时生效；不信任客户端提供的 `X-Forwarded-For`。通过同源反向代理部署时，后端看到的是代理 IP，经过该代理的请求会共享额度。生产入口应按可信的真实客户端 IP 限流，并在多个后端副本之间统一执行限制；后端阈值需结合代理汇总流量配置。重启进程会清空本机限流窗口。

## 检查与测试

```sh
make api-docs-check
make check
make check-schema
SILVA_TEST_DATABASE_URL=postgres://silva:silva@127.0.0.1:55439/silva make test-db
make worker-once
```

`make check` 执行格式、Clippy（警告视为错误）、Workspace 构建和普通测试，其中包括 Rust OpenAPI 契约、Schema 及示例校验。`make api-docs-check` 可单独运行契约测试。`make test-db` 显式执行数据库集成测试，并按契约校验真实接口的状态、媒体类型、响应头和 JSON Schema：连接角色需有 `CREATEDB`，测试创建独立临时数据库并在结束后清理。缺少测试 URL 或数据库不可用会失败，不把数据库测试静默当作成功。

`check-schema` 在迁移后的真实数据库检查业务表/字段非空注释及 UUIDv7 约束；测试覆盖 UUIDv7 写入、v4/错误变体拒绝和 API 非 v7 ID 拒绝，以及登录、CSRF、RBAC、账户停用和会话撤销。GitHub Actions 使用临时 PostgreSQL 执行相同门禁。

所有业务实体、事件、审计和 request ID 都通过 `silva_shared::ids` 生成 UUIDv7。新增或变更数据库字段时，在同一次迁移维护 `COMMENT ON TABLE/COLUMN`；已执行的迁移不修改，追加新迁移。

数据库当前保留 31 张业务表。旧开发库执行 `make migrate` 后会删除已取消的目录功能对应的四张表及其数据，再运行 `make check-schema` 验证；历史迁移保留原校验和。

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
