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

| 变量               | 说明                                                                 | 默认值                                                                  |
| ------------------ | -------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `SILVA_API_TARGET` | Vite 开发代理指向的 Rust API 地址                                    | `http://127.0.0.1:8088`                                                 |
| `VITE_WEBSITE_URL` | 「查看站点」及已上线文章链接的公开地址；分域部署时设为读者使用的域名 | 开发环境 `http://localhost:4321`（`pnpm dev`），生产环境使用当前 origin |

使用 Compose 网关时，在 `apps/admin/.env.local` 配置三个上游。管理请求、公开站预览和系统健康页面仍使用浏览器同源路径。

```dotenv
SILVA_API_TARGET=http://127.0.0.1:8089
SILVA_PUBLIC_API_TARGET=http://127.0.0.1:8088
SILVA_OPERATIONS_TARGET=http://127.0.0.1:8090
VITE_WEBSITE_URL=http://localhost:8088
```

宿主机直接运行 Rust API 时，公开与运维代理默认沿用 `SILVA_API_TARGET`。

## OpenAPI 类型同步与校验

```sh
pnpm generate:api   # 从 server/apps/api/openapi.json 重新生成类型
pnpm check:api      # 校验生成结果与 OpenAPI 契约是否一致
pnpm check          # format:check + lint + typecheck
pnpm test           # 运行 Vitest 单元与组件测试
pnpm build:admin    # 构建生产产物到 apps/admin/dist
```
