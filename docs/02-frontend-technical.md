# Developer Hub 前端技术文档

- 版本：1.2
- 技术方向：Astro Website + React Admin SPA + TypeScript API Client
- 后端接口：Rust Axum REST API
- 数据访问：前端只调用 API，不直连数据库

## 1. 前端总体架构

采用独立前台和管理后台：

- `apps/website`：Astro 公共站点，静态优先。
- `apps/admin`：React 管理后台 SPA。
- `packages/api-client`：由 OpenAPI 生成的 TypeScript API Client。
- `packages/ui`：可复用 UI 组件。
- `packages/shared`：共享类型、常量、校验和工具。

建议使用 pnpm workspace + Turborepo（初期也可只用 pnpm workspace）。

```text
frontend/
├── apps/
│   ├── website/
│   │   ├── src/
│   │   │   ├── components/
│   │   │   ├── layouts/
│   │   │   ├── pages/
│   │   │   ├── islands/
│   │   │   ├── content/
│   │   │   └── styles/
│   │   └── astro.config.mjs
│   └── admin/
│       └── src/
│           ├── app/
│           ├── routes/
│           ├── features/
│           ├── components/
│           ├── hooks/
│           ├── lib/
│           └── styles/
├── packages/
│   ├── api-client/
│   ├── ui/
│   └── shared/
├── package.json
└── pnpm-workspace.yaml
```

## 2. 技术选型

### 2.1 Website

- Astro：页面路由、静态生成、布局和内容页面。
- React Islands：仅用于确实需要交互的组件。
- TypeScript：业务类型。
- Tailwind CSS：样式系统（可替换）。
- Zod：表单或客户端输入校验。
- OpenAPI-generated client：调用 Axum API。

公开站点采用 Static-first：

- 首页、文章、分类、标签、About 和 AI Trending 初始榜单使用 SSG。筛选、分页和趋势图按需调用 Public API。
- 一期不实现站内搜索、收藏或访客登录，不因少量动态区块引入 SSR。

### 2.2 Admin

- React + Vite + TypeScript
- TanStack Router：路由和路由级加载
- TanStack Query：服务端状态、缓存和失效
- React Hook Form + Zod：表单
- shadcn/ui + Tailwind：组件基础
- CodeMirror 6：Markdown 编辑器
- OpenAPI-generated TypeScript Client：接口调用

### 2.3 版本策略

使用项目初始化时稳定且相互兼容的版本，并通过 lockfile 固定。不要在文档中把某个小版本写成长期不变的约束。

## 3. Website 模块设计

```text
src/
├── pages/
│   ├── index.astro
│   ├── blog/
│   ├── categories/
│   ├── tags/
│   ├── ai-trending/
│   └── about.astro
├── components/
│   ├── layout/
│   ├── navigation/
│   ├── content/
│   ├── cards/
│   └── seo/
├── islands/
│   ├── AiTrendingFilters.tsx
│   └── AiTrendingChart.tsx
├── layouts/
└── lib/
    ├── api.ts
    ├── seo.ts
    └── format.ts
```

### 3.1 公开页面数据策略

- Worker 向 Astro 提供指定 `generation` 的不可变构建快照。页面、分类/标签、站内链接、404、sitemap 与 RSS 都从这份快照生成；构建不逐页读实时 Public API，不直连数据库。
- 构建校验快照的 `generation`、schema 版本与完整性；快照读取或校验失败时停止，不回退到其他版本或空数据。没有内容或没有成功 AI 批次是有效快照，两者需与加载失败区分。
- AI Trending 的筛选、分页及图表通过当前 Public API 获取。静态页面记录自身 generation；动态请求携带 `X-Expected-Generation`，遇到 `409 site_generation_changed` 时清空该页面缓存的旧数据并提示刷新，避免拼接两个版本。
- 显示 AI 榜单数据日期；`null` 增长渲染为「数据不足」，保留负值。将精选与自然排名分开展示；尚无成功批次显示「暂无数据」，不把采集失败渲染为零增长。

### 3.2 SEO

每个详情页提供 title、description、canonical、Open Graph、Twitter metadata。
文章使用 Article/BlogPosting JSON-LD。个人资料可使用 Person。
生成 sitemap、RSS、robots.txt。生产入口对快照不存在的详情路由返回真实 HTTP 404，不回落到旧产物；文章 slug 首发接受后锁定，改名及旧 slug 重定向不在一期。

## 4. Admin 模块设计

```text
src/features/
├── auth/
├── dashboard/
├── posts/
│   ├── list/
│   ├── editor/
│   ├── revisions/
│   └── reviews/
├── taxonomy/
├── media/
├── profile/
├── ai-trending/
│   ├── repositories/
│   ├── snapshots/
│   └── ingestion-runs/
├── publishing/
├── users/
├── roles/
└── audit-logs/
```

### 4.1 内容编辑器

文章编辑器需要：

- Markdown 源码编辑与预览。
- 自动保存（带 debounce 和并发版本检测）。
- 分别显示当前编辑 revision、期望公开 revision 和当前线上 revision；后两者可能不同。线上状态来自服务端，不由「发布」请求是否成功推断。
- 在管理域使用未发布 revision 做安全预览，不生成公开预览 URL。
- 提交审核、批准、拒绝都绑定明确的 revision；发布仅可选择已批准 revision，另提供取消发布。
- 编辑分类标签、封面、SEO 和首页精选标记；这些字段随 revision 发布。
- 上传先向 Admin API 申请签名，再直传私有对象存储，最后调用确认接口。仅在服务端返回 `ready` 后插入媒体引用；失败保留编辑内容并提示。
- 保存失败时保留本地编辑状态。

每次保存由后端创建不可变 revision。保存传 `expected_revision_id`；收到 `409 revision_conflict` 时停止自动保存、保留本地草稿并提示读取最新记录，不盲重试。发布和取消发布传 `expected_publication_version`；若冲突，重新读取文章与发布状态后再由用户确认。

### 4.2 AI Trending 管理界面

AI Trending 是自动采集产品，不设计成手动录入榜单。后台主要功能：

- Repository 列表和详情。
- 数据源/采集任务运行状态。
- 最近一次成功采集时间、错误和限流信息。
- 快照历史和增长曲线。
- 支持隐藏或恢复仓库、修正分类和标记精选。保存后与其他内容走同一上线流程。
- Admin 可请求当日未成功批次；已运行或已完成时显示现有批次与状态，不创建重复批次。

### 4.3 站点发布任务

提交文章发布或取消发布、保存公开分类标签、个人资料或 AI 榜单展示设置后，先显示「待上线」。根据返回的 job 或 generation 查询任务：`queued` / `building` 显示进度，`active` 表示已上线，`failed` 显示失败原因，`superseded` 表示被新的目标取代，`retired` 表示曾上线但已被替换。任务被取代不代表变更被撤销，应重新读取当前目标与线上状态。超过 5 分钟仍待上线时提示延迟；Admin 可重试仍为最新目标的失败 generation。发生构建失败时不要把编辑或保存状态标成失败。

## 5. API Client 与请求约定

- 由后端 OpenAPI 文档生成 TS Client，禁止手写重复 DTO。
- 本站业务实体 ID 是由服务端生成的 UUID v7（UUIDv7）字符串。前端原样传递 API 返回的 ID，不自行生成或转成数字；slug、外部来源 ID 和发布 generation 保持各自的类型。
- Admin 浏览器请求使用同源相对路径 `/api/v1/admin`，由入口代理到 Axum；本地开发由 Vite 代理。公开页面浏览器请求同源 `/api/v1/public`，不带管理会话。
- Admin 使用 Cookie Session；除登录外的写操作发送 `X-CSRF-Token`，由登录响应或 `auth/me` 提供，仅保存在内存并随会话刷新。登录自身校验同源 Origin 和 JSON Content-Type。
- 统一处理 401、403、409、422、429 和 5xx，并根据 `error.code` 区分 revision 冲突、发布版本冲突、站点版本变化等。
- TanStack Query key 按资源域组织，例如 `['posts', filters]`。
- 写操作成功后精确 invalidate 相关 query。
- 对列表接口统一支持 `page`、`per_page`、`sort`、`order` 和过滤参数。

构建器通过非公开配置读取 Worker 提供的只读快照文件及预期 generation；生产代理和开发代理的上游地址各自由部署配置。不要把内部地址、服务端密钥或数据库凭据打进客户端产物。

## 6. 登录与安全

- Admin 使用 host-only 的 HttpOnly、Secure、SameSite=Lax Session Cookie；Cookie 不跨到公共站域名。服务端对后台写操作校验 Origin 和 CSRF（登录例外见 §5）。
- 前端不在 localStorage 保存长期 bearer token。
- 按后端 RBAC 控制导航和按钮显示，但真正授权必须由后端执行。
- Markdown 预览应使用安全渲染策略；不信任未经消毒的 HTML。
- 上传限制文件大小、MIME、扩展名，并由服务端重新校验。

## 7. 状态管理边界

- 服务端数据：TanStack Query。
- 表单状态：React Hook Form。
- 短生命周期 UI 状态：组件 state。
- 跨页面、非敏感偏好：必要时使用轻量状态库。
- 不复制一份全局 store 来缓存所有 API 数据。

## 8. 测试与质量

- Vitest：单元测试。
- Testing Library：组件测试。
- Playwright：登录和越权拒绝、自动保存冲突保留输入、已上线旧版不受新草稿影响、发布/取消发布状态与 404、上传未确认不能插入、个人资料保存与上线、榜单空态与缺样。
- ESLint + Prettier + TypeScript strict。
- 对 API Client 做生成检查，确保 OpenAPI 变化后类型同步。

## 9. 前端交付顺序

1. Workspace、设计系统、API Client。
2. Admin 登录、布局、权限路由。
3. Posts 列表、编辑器、媒体确认、Revision、审核与发布任务。
4. Website 首页、Blog、SEO。
5. 个人资料管理与 About 页面展示。
6. AI Trending 仪表盘、趋势页面和交互筛选。
7. E2E、性能和可访问性优化。
