# Silva 公开站点

公开站点位于 `apps/website`，采用 Astro 静态生成。文章与个人资料读取本地 Markdown，AI 榜单和趋势图使用标注为示例的本地快照。开发和构建可直接运行。

## 本地运行

使用 Node.js 24 LTS 和 pnpm 11。在本目录运行：

```sh
pnpm install --frozen-lockfile
pnpm dev
```

开发服务器默认地址为 `http://localhost:4321/`。

```sh
pnpm build
pnpm preview
```

生产静态产物位于 `apps/website/dist`。正式构建时，用 `SITE_URL` 指定公开站点的域名，供 canonical、RSS 和站点地图使用。默认本地预览使用 localhost，并在页面输出 `noindex`。静态主机应将未知路径交给 `404.html` 并返回 HTTP 404。

## 代码校验

- `pnpm format` 使用 Oxfmt 格式化文件，`pnpm format:check` 只检查格式。
- `pnpm lint` 使用 Oxlint 检查代码，`pnpm lint:fix` 应用可自动修复的问题。
- `pnpm typecheck` 使用 Astro Check 检查模板和类型。
- `pnpm check` 依次运行格式检查、Lint 和类型检查。

Oxfmt 覆盖 Astro 模板、TS/TSX、CSS、Markdown 和配置文件。Oxlint 检查 JS/TS、React 和 Astro 的脚本，Astro 模板诊断由 Astro Check 提供。

## 内容位置

- `apps/website/src/content/posts/` 包含示例文章。文件名决定文章 slug，分类和标签来自 `src/data/taxonomy.ts`。
- `apps/website/src/content/profile/owner.md` 保存个人简介、经历、技能和公开链接。
- `apps/website/src/data/trending.ts` 提供示例榜单和历史快照。近 7 天和 30 天增长从快照计算。
