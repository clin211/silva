# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目约定（来自 AGENTS.md）

- `server/` 中所有代码注释必须用中文，包括 Rust 文档注释、测试说明、SQL 注释和配置说明；技术术语、标识符和工具要求的语法保留原文。
- 开发与校验优先使用 Rust 生态工具，不为 API 文档引入 Node.js 工具链。API 文档以 `apps/api/openapi.json` 交付，用 Apifox 预览，服务端不内嵌预览页面。
- 提交信息沿用 `feat(backend): 中文标题` + 要点列表的格式。

## 常用命令

所有命令在 `server/` 下执行，工具链固定为 Rust 1.100.0（`rust-toolchain.toml`）。

```sh
cp .env.example .env && make db-up && make migrate && make check-schema   # 首次准备本地库（127.0.0.1:55439，silva/silva/silva）
make api / make worker / make worker-once                                 # 运行 API（127.0.0.1:8088）/ Worker / 单次维护
make check            # fmt --check + clippy -D warnings + build + 全部非数据库测试（CI 门禁同此）
make api-docs-check   # 只跑 OpenAPI 契约测试（apps/api/tests/openapi.rs）
SILVA_TEST_DATABASE_URL=postgres://silva:silva@127.0.0.1:55439/silva make test-db   # 数据库集成测试
```

- 单个测试：`cargo test -p silva-api --test openapi <测试名>`；单元测试如 `cargo test -p silva-shared <测试名>`。
- 数据库集成测试 `apps/api/tests/identity.rs` 标记为 `#[ignore]`，必须加 `-- --ignored` 并设置 `SILVA_TEST_DATABASE_URL`；它会创建并清理独立的临时数据库，连接角色需要 `CREATEDB`。缺少 URL 时测试失败，而不是跳过。
- 管理员初始化：`SILVA_ADMIN_PASSWORD` 只能通过环境变量提供，`cargo run --locked -p silva-cli -- seed-admin --email ... --display-name ...`。
- 观测栈：`make observability-up`、`make observability-check`、`make observability-smoke`（详见 `observability/README.md`）。

## 架构

模块化单体：一个 Cargo Workspace，三个独立运行的 binary 共用领域与用例代码。

- `crates/shared`：配置（`Config::load`，读取 `.env`）、`AppError`、UUIDv7 生成（`ids`）、分页、Prometheus 指标、OpenTelemetry 遥测。所有实体 ID、请求 ID、审计 ID 都通过 `silva_shared::ids` 生成 UUIDv7。
- `crates/domain`：实体、内置角色到权限的映射（`permissions_for_role`）、`Principal::require` 权限检查。不依赖数据库和 HTTP。
- `crates/application`：用例（目前是 `IdentityService`），只依赖 `ports.rs` 中的 `IdentityRepository` 和 `Credentials` trait。
- `crates/infrastructure`：这些端口的实现（SQLx 的 `PgIdentityRepository`、Argon2id 的 `CryptoCredentials`），以及迁移、`check_schema` 和 `identity_service()` 装配函数。
- `apps/api`：Axum 路由（`lib.rs::build_router`）。`http.rs` 中的 `context` 中间件统一处理 request ID / trace ID、限流、写操作的 Origin 校验、`Cache-Control: no-store` 和指标；鉴权和 CSRF 在各个处理器里调用 `authorize()` 完成，不是中间件。
- `apps/worker`：定时会话维护，并在 `SILVA_WORKER_METRICS_BIND` 上暴露 `/metrics`。
- `apps/cli`：`migrate`、`check-schema`、`seed-admin`。
- `xtask`：观测栈冒烟和故障演练（`silva-xtask`）。

一个请求的路径：Axum 提取器（`Input`、`PageInput`、`IdPath`）→ 处理器 `authorize()` → `IdentityService` 用例 → 端口 trait → PostgreSQL。错误按 `AppError` → `ApiError` 的方向转换，HTTP 状态只在 API 层决定。

## 响应与 API 契约

- 必须遵守 `docs/contracts/api-response.md`。成功响应为 `{"data": ...}`，分页另带 `{"meta": {"pagination"}}`，204 没有正文；失败响应为 `{"error": {code, message, details, request_id}}`。
- 2xx 响应只能通过 `apps/api/src/response.rs` 中的函数构造。错误码登记在 `apps/api/src/error.rs` 的 `ErrorCode` 中，它唯一决定 HTTP 状态。字段级错误使用 `AppError::Validation(Vec<FieldError>)`，必须写明具体字段。
- `apps/api/openapi.json` 是手工维护的唯一契约，运行时通过 `/openapi.json` 用 `include_str!` 原样返回。修改接口时，要同步更新路径、Schema、`x-required-permission`、`x-error-codes` 和示例。契约测试会校验：错误码枚举与代码一致；每个声明的路由都已注册；示例符合 Schema。数据库集成测试会按这份 Schema 校验每个真实响应。
- 接口变化同步到 `docs/api/README.md`（接入指南）和 `docs/contracts/identity.md`。

## 安全与身份约束

- 会话放在 `__Host-silva_session` Cookie 中（HttpOnly、Secure、SameSite=Lax、Path=/、不设 Domain），数据库只保存令牌哈希；不接受 Bearer Token。
- `/api/v1/admin/*` 的写操作要求 `Origin` 精确等于 `SILVA_ADMIN_ORIGIN`；登录之外的写操作还要求与会话绑定的 `X-CSRF-Token`。没有启用 CORS。
- 授权在每次请求时按账户当前的状态和角色计算，不使用登录时的权限快照。停用账户或管理员重置密码时，撤销该账户的全部会话；修改自己的密码时，只保留当前会话。
- 不能移除最后一个启用状态的管理员（返回 409 `last_admin_required`，用 advisory lock 串行化）。
- 限流在进程内按直连 peer IP 计算（IPv6 按 /64 合并），不信任 `X-Forwarded-For`。登录和修改密码共用登录额度。
- 审计不得记录密码、哈希、会话令牌或 CSRF Token。

## 数据库

- 迁移文件在 `migrations/`，由 SQLx 执行。已执行过的迁移不能修改，变更一律追加新迁移。
- 每张业务表和每个字段都必须在同一个迁移里写 `COMMENT ON TABLE/COLUMN`；实体 ID 列用 `CHECK (is_uuid_v7(...))` 约束。`make check-schema` 和 CI 会检查这两点。
- 数据库已预建后续里程碑（CMS、媒体、发布、采集）的表结构，但这些业务尚未实现。不要在 OpenAPI 中承诺未实现的接口。完整 DDL 和约束说明见 `docs/contracts/database.md`，总体设计见仓库根目录的 `docs/03-backend-technical.md`。

## Skills

Rust 相关的 skill 存放在 `server/.agents/skills/`，`server/.claude/skills/` 中是指向它们的软链接。处理 Rust 问题时，先用 `rust-router` 判断应该使用哪个 skill。
