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

## 快照模式

正式站点由后端 Worker 导出的不可变 JSON 快照构建，契约见 [`server/docs/contracts/site-snapshot.md`](../server/docs/contracts/site-snapshot.md)。设置 `SILVA_SNAPSHOT` 即进入快照模式；未设置时使用仓库内的示例内容。

| 变量                        | 含义                                            |
| --------------------------- | ----------------------------------------------- |
| `SILVA_SNAPSHOT`            | 快照文件的绝对路径                              |
| `SILVA_SNAPSHOT_SHA256`     | 快照文件字节的 SHA-256（小写十六进制）          |
| `SILVA_SNAPSHOT_GENERATION` | 期望的 generation                               |
| `SILVA_SITE_OUT_DIR`        | 构建输出目录，默认 `./dist`                     |
| `SITE_URL`                  | 公开站点 origin，用于 canonical、RSS 和站点地图 |

构建开始前依次校验 SHA-256、`schema_version` 和 `generation`，再按契约校验结构和引用关系，任一失败即以非零状态退出，不回退到示例内容。每个 HTML 页面输出 `<meta name="silva:generation" content="…">`。正文代码块在构建时用 Shiki 着色。

Astro 用 `rename` 移动构建资源，`SILVA_SITE_OUT_DIR` 须与 `apps/website` 位于同一文件系统。

用示例快照 `apps/website/fixtures/snapshot.sample.json` 验证快照模式：

```sh
pnpm --filter @silva/website build:snapshot-fixture
```

脚本计算 SHA-256 并构建到临时目录，输出路径打印在开头。

页面统一通过 `src/lib/data.ts` 读取文章、分类、标签、个人资料和 AI 榜单，快照的读取与校验在 `src/lib/snapshot.ts`。

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
