# Silva 管理后台 (`@silva/admin`)

Silva 后台管理系统 SPA，基于 React 19 + Vite + TypeScript + TanStack Router + TanStack Query + React Hook Form + Zod + Tailwind CSS + Tiptap (`@tiptap/markdown`)，接口类型完全由 `server/apps/api/openapi.json` 自动生成到 `@silva/api-client`。

## 本地开发

在 `frontend/` 目录下运行：

```sh
pnpm install
pnpm dev:admin
```

默认运行在 `http://localhost:5173`，Vite 自动将 `/api`、`/health`、`/metrics` 代理到 `SILVA_API_TARGET`（默认 `http://127.0.0.1:8088`）。

## 环境变量

| 变量               | 说明                                 | 默认值                  |
| ------------------ | ------------------------------------ | ----------------------- |
| `SILVA_API_TARGET` | Vite 开发代理指向的 Rust API 地址    | `http://127.0.0.1:8088` |
| `VITE_WEBSITE_URL` | 顶部「查看站点 ↗」跳转的公开站点地址 | `http://localhost:4321` |

## OpenAPI 类型同步与校验

```sh
pnpm generate:api   # 从 server/apps/api/openapi.json 重新生成类型
pnpm check:api      # 校验生成结果与 OpenAPI 契约是否一致
pnpm check          # format:check + lint + typecheck
pnpm test           # 运行 Vitest 单元与组件测试
pnpm build:admin    # 构建生产产物到 apps/admin/dist
```
