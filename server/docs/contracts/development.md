# 开发与部署契约

来源：`docs/03-backend-technical.md` §2、§3.1、§3.2、§9、§10、§11；主线程约定的工具链和命令。

## 本切片范围

`server/` 是 Rust 2024 Workspace，工具链为 `1.100.0`。API、Worker、CLI 的 package 和 binary 名分别为 `silva-api`、`silva-worker`、`silva-cli`。共享 crate 为 `silva-shared`、`silva-domain`、`silva-application`、`silva-infrastructure`。API 与 Worker 独立运行，CLI 用于一次性运维；本阶段交付里程碑 1 的开发基础设施和里程碑 2 中的媒体直传与确认，不宣称 CMS、发布或采集业务已经实现。

`xtask/` 提供 `silva-xtask` Rust binary，负责本地告警接收、观测冒烟校验和故障演练，纳入 Workspace 检查。观测栈无需 Python 运行时。

## 容器契约

- 多阶段 Docker 构建，使用同一锁文件构建 release binary；默认运行镜像包含 API、Worker、CLI、CA 证书并使用非 root 用户。
- `observability-tools` 阶段额外包含 `silva-xtask`，供本地告警接收器使用；默认 `backend` 阶段保持三个业务 binary。
- API 默认启动 `silva-api`，Worker 覆盖启动命令为 `silva-worker`，CLI 通过覆盖命令执行。
- PostgreSQL 使用 17 系列镜像。Compose 数据库监听仅绑定本机，持久化具名卷并提供 `pg_isready` 健康检查。
- Compose 将迁移作为独立一次性服务；API 和 Worker 在数据库健康、迁移成功后启动，不并发执行启动迁移。
- API 仅发布本地开发端口。生产需 HTTPS 入口、Admin 同源 `/api/v1/admin` 代理和按域限制路由，不启用管理跨域凭证。
- 对象存储直接使用 Cloudflare R2 或阿里云 OSS 的私有 bucket，Compose 不内置本地对象存储服务；未配置时开发环境的媒体接口返回 503。不暴露内部任务完成接口。

## 配置契约

配置由 `server/.env.example` 和 `crates/shared/src/config.rs` 冻结：`DATABASE_URL`、`SILVA_BIND`、`SILVA_ENV`、`SILVA_DB_MAX_CONNECTIONS`、`SILVA_JWT_SECRET`、`SILVA_ACCESS_TOKEN_TTL_SECONDS`、`SILVA_REFRESH_TOKEN_TTL_SECONDS`、`SILVA_WORKER_INTERVAL_SECONDS`、`SILVA_API_RATE_PER_MINUTE`、`SILVA_LOGIN_RATE_PER_MINUTE`、`SILVA_LOGIN_LOCKOUT_THRESHOLD`、`SILVA_LOGIN_LOCKOUT_SECONDS`、`SILVA_STORAGE_PROVIDER`、`SILVA_STORAGE_ENDPOINT`、`SILVA_STORAGE_BUCKET`、`SILVA_STORAGE_REGION`、`SILVA_STORAGE_ROOT`、`SILVA_STORAGE_ACCESS_KEY_ID`、`SILVA_STORAGE_ACCESS_KEY_SECRET`、`SILVA_MEDIA_UPLOAD_TTL_SECONDS`、`RUST_LOG`。

对象存储：`SILVA_STORAGE_PROVIDER` 为 `r2` 或 `oss`，设置后 endpoint、bucket 和两个密钥均必填，`oss` 还必须设置 `SILVA_STORAGE_REGION`（V4 签名）；`SILVA_MEDIA_UPLOAD_TTL_SECONDS` 不超过 86400；生产环境 API 未配置时启动失败，且只接受 HTTPS endpoint。访问密钥的 Debug 输出脱敏，不写入日志。Worker 未配置时跳过媒体清理，CLI 不使用。

宿主机开发默认 PostgreSQL `127.0.0.1:55439`、API `127.0.0.1:8088`。Compose 覆盖数据库主机为 `postgres`，容器 HTTP bind 为 `0.0.0.0:8088`；宿主机端口仍仅绑定 `127.0.0.1`。管理接口使用 `Authorization: Bearer <access_token>`；生产环境必须设置至少 32 字节的 `SILVA_JWT_SECRET`，否则 API 启动失败，开发和测试环境未设置时 API 随机生成密钥并告警（重启后令牌失效）。CLI 与 Worker 不签发令牌，不要求该变量。

CLI 管理员密码只通过 `SILVA_ADMIN_PASSWORD` 提供，不出现在命令参数、仓库或日志中；邮箱/名称通过 `--email`/`--display-name` 或 `SILVA_ADMIN_EMAIL`/`SILVA_ADMIN_NAME`。

## 命令契约

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace --locked`
- `cargo run -p silva-cli -- migrate`
- `cargo run -p silva-cli -- check-schema`：必须在真实迁移后的数据库检查表/列非空注释及 UUIDv7 约束。
- `cargo run -p silva-cli -- seed-admin --email admin@example.com --display-name Administrator`：密码读取 `SILVA_ADMIN_PASSWORD`。
- `cargo run -p silva-cli -- create-user --email author@example.com --display-name Author --role author`：创建指定内置角色的账户，`--role` 可重复（`author`、`editor`、`admin`）；密码读取 `SILVA_USER_PASSWORD`。审计动作与 `seed-admin` 相同，为无操作账户的 `users.bootstrap`。
- `cargo run -p silva-worker -- --once`：执行一次真实维护清理（过期会话、过期未确认与保留期已满的已拒绝媒体）后退出，不声称处理尚未实现的发布或采集任务。
- `cargo test -p silva-api --test identity --test media -- --ignored`：读取 `SILVA_TEST_DATABASE_URL`，连接角色需 CREATEDB，用独立临时数据库测试后清理。CI URL 为 `postgres://silva:silva@localhost:5432/silva_test`。

- `cargo test -p silva-infrastructure --test object_storage_live -- --ignored`：读取 `SILVA_STORAGE_*` 连接真实 R2 或 OSS bucket，验证预签名直传、禁止覆盖写入、读取与删除；不在 CI 中运行。

Makefile 封装构建、格式、静态检查、测试、数据库启动、迁移、schema 检查、开发 API/Worker 和容器构建。README 给出可复制的首次启动、管理员初始化、健康检查和登录请求示例。

## CI 门禁

GitHub Actions 仅在 `server/**` 或后端 workflow 变更时执行。临时 PostgreSQL 服务使用独立测试库，凭据仅为 CI 本地测试值。固定工具链安装 rustfmt/clippy，缓存 Rust 编译产物。

检查顺序：格式、Clippy、构建、真实数据库迁移、schema 检查、全部测试与数据库集成测试。必须验证迁移后的业务表及未删除用户字段注释；UUIDv7 可写入，v4 和错误变体被拒绝；API 拒绝非 v7 ID；会话与 RBAC 测试真实执行而非忽略或跳过。命令失败直接阻断 CI。

## 后续验收锚点

主线程负责真实重跑编译和测试并抽查容器/CI 文件。运行能力具备时验证 `docker compose config`、镜像构建、数据库迁移与 API HTTP 冒烟。当前尚未创建容器或修改外部服务。
