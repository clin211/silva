# Silva 服务端

@AGENTS.md

## Workspace

服务端使用 Rust、Axum、Tokio、SQLx 和 PostgreSQL。工具链版本由 `rust-toolchain.toml` 固定。

- `apps/api` 提供 HTTP 路由、认证、DTO 和 OpenAPI 契约。业务用例放在 `crates/application`。
- `apps/worker` 运行后台维护任务，`apps/cli` 提供数据库迁移、schema 检查和管理员初始化。
- `crates/domain` 放纯领域规则，`crates/infrastructure` 实现数据库等适配器，`crates/shared` 提供配置、错误、ID 和日志等共用能力。
- `migrations` 存放 SQLx 迁移。已执行的迁移保留原校验和，schema 变更通过追加迁移交付，同步维护表和列的中文注释。
- 业务实体、事件、审计和请求 ID 通过 `silva_shared::ids` 生成 UUIDv7。

## 开发与验证

在 `server/` 下运行命令。完整的本地配置和启动步骤见 [README.md](README.md)。

- `make check` 检查格式、运行 Clippy、构建 Workspace 并运行常规测试。
- `make api-docs-check` 单独验证 OpenAPI 契约与示例。变更 API 时同步更新 `apps/api/openapi.json` 和相关契约测试。
- `make db-up` 启动开发数据库，`make migrate` 执行迁移，`make check-schema` 验证迁移后的 schema。
- `make test-db` 用 `SILVA_TEST_DATABASE_URL` 指向独立的开发或测试数据库运行集成测试。连接角色需要 `CREATEDB`。
- `make api` 和 `make worker` 分别启动 API 和 Worker。
