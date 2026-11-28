# Silva 公开站点

公开站位于 `apps/website`。Astro 在请求时从 Rust API 读取已发布内容，生成文章、归档、专栏、首页、RSS 和站点地图。编辑草稿期间，访客继续看到上次发布的内容。

## 本地运行

准备 Node.js 24 LTS 或仓库 `.node-version` 指定的版本，以及 pnpm 11。先按 [后端说明](../server/README.md) 启动 PostgreSQL 和 Rust API，再在 `frontend/` 运行以下命令。

```sh
pnpm install --frozen-lockfile
pnpm dev
```

在 `http://localhost:4321/` 查看站点。本地开发默认从 `http://127.0.0.1:8088` 读取公开数据，并将同源 `/api/v1/public/` 请求转发到 Rust。后端端口不同时，在 `apps/website/.env` 中将 `SILVA_PUBLIC_API_ORIGIN` 设为实际 API 地址，`SITE_URL` 设为浏览器访问的站点地址。修改后重新启动开发或预览服务。

```sh
pnpm build:website
pnpm preview
```

预览使用编译好的 SSR 服务，也会请求当前已发布的数据，并提供同源公开媒体。`pnpm build` 同时构建公开站与管理后台。在 `frontend/` 执行 `pnpm --filter @silva/website test:ssr` 可运行 HTTP 回归测试。

## 线上公开站

`server/` 的 `make up` 通过同一网关提供 Astro 页面和 Rust API。单独启动编译后的 Node 服务可使用以下命令。

```sh
cd apps/website
HOST=127.0.0.1 PORT=4321 node --env-file-if-exists=.env dist/server/entry.mjs
```

将 `SITE_URL` 设置为访客实际访问的 origin，以生成正确的 canonical、RSS 和 sitemap 链接。网关将 `/api/` 转发到 Rust，页面与静态资源交给 Astro。Astro 默认请求 `/api/v1/public/site-snapshot`，Compose 通过 `SILVA_SITE_SNAPSHOT_PATH` 配置服务间的内部路径。页面按 ETag 核对版本，API 不可用时返回 HTTP 503。数据字段见[站点快照契约](../server/docs/contracts/site-snapshot.md)。

## 代码校验

- `pnpm format` 使用 Oxfmt 格式化文件，`pnpm format:check` 只检查格式。
- `pnpm lint` 使用 Oxlint 检查代码，`pnpm lint:fix` 应用可自动修复的问题。
- `pnpm typecheck` 使用 Astro Check 检查模板和类型。
- `pnpm check` 依次运行格式检查、Lint 和类型检查。

Oxfmt 覆盖 Astro 模板、TS/TSX、CSS、Markdown 和配置文件。Oxlint 检查 JS/TS、React 和 Astro 的脚本，Astro 模板诊断由 Astro Check 提供。
