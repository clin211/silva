# Silva 公开站点设计系统 v2

由 `ui-ux-pro-max` 生成，查询词为 `developer portfolio tech blog personal --design-system`，再加上 style、typography、color 三个方向的补充查询，按下面的取舍综合而成。实现位于 `assets/site.css`、`assets/site.js` 和 `assets/tw-config.js`（`font-display`、`font-body`、`font-mono`）。

## 取舍

| 项目 | skill 推荐 | 采用 | 原因 |
| ---- | ---------- | ---- | ---- |
| 风格 | Brutalism | **Bento Box Grid** | 粗野主义不适合以阅读为主的中文技术站。Bento 能用不同尺寸的模块组织首页和列表的层次，在 style 补充查询中排第一 |
| 配色 | Monochrome + blue accent | 采用 | 符合开发者站点，和代码块、数据榜单搭配自然 |
| 字体 | Caveat / Quicksand | **Space Grotesk / Inter / JetBrains Mono** | 手写体不适合技术内容。采用 typography 补充查询中的 Tech Startup 与 Developer Mono 组合；中文统一回退到 Noto Sans SC |
| 动效 | Stagger List | 只在首屏使用一次 | 列表和网格只在悬停时有 150–200ms 的反馈；开启「减少动态效果」时不播放 |

## 令牌

| 令牌 | 值 | 用途 |
| ---- | -- | ---- |
| `--bg` | `#FAFAFA` | 页面底色 |
| `--card` | `#FFFFFF` | 卡片 |
| `--fg` | `#09090B` | 标题、正文主色 |
| `--muted` | `#52525B` | 次要文字（对白底对比度 7.7:1） |
| `--border` | `#E4E4E7` | 卡片边框；悬停时用 `#D4D4D8` |
| `--accent` | `#2563EB` | 链接、强调、选中状态 |
| `--up` / `--down` | `#15803D` / `#DC2626` | 增长、负增长，始终配合 + 或 − 号，不只靠颜色区分 |
| 圆角 | 卡片 20px；内部元素 12–16px；按钮 12px；标签 6px | |

## 组件类（`site.css`）

- `.tile`：白底、细边框、20px 圆角的 Bento 卡片。可点击的卡片再加 `.tile-link`，悬停时上浮 2px 并加深阴影。
- `.reveal`：容器的子元素依次入场。每个子元素用 `style="--i:n"` 指定顺序，只在首屏使用。
- `.btn`：高度 44px 的按钮，有 `.btn-primary`（黑）、`.btn-accent`（蓝）、`.btn-ghost`（描边）三种。
- `.tag` / `.tag-accent`：技术栈、分类标签。状态用 `base.css` 里的 `.chip-*`，文字和颜色同时出现。
- `.seg`：分段切换，用于时间窗口、视图切换等。选中项用 `aria-pressed`、`aria-current` 或 `aria-selected` 标记。
- `.field`：筛选下拉框和输入框，高 40px。
- `.prose-site`：长文排版，正文最大宽度 720px，代码块为深色。
- `.cover`：封面占位，浅蓝渐变加点阵。
- `.dot-grid` / `.line-grid`：装饰底纹，只用在少数模块上。
- `.section-title`：区块标题，Space Grotesk 粗体，字距收紧。
- 图标：在 HTML 里写 `<i data-icon="名称" class="w-4 h-4"></i>`，由 `site.js` 换成 Lucide SVG；在 JS 里用 `icon(名称, 类名)`。不使用 emoji 当图标。

## 页面骨架

- `<body class="site antialiased" data-nav="…">`，然后依次是 `#site-header`、`<main id="main" class="max-w-site mx-auto px-4 md:px-6">` 和 `#site-footer`。
- 页面标题区：面包屑（如需要）、`h1`（Space Grotesk，32–48px）、一句说明，下面接 Bento 内容区。
- 断点：小于 768px 单列；768–1023px 两列卡片；1024px 以上用双栏或 Bento 跨列。手机两侧各留 16px，不出现整页横向滚动。
- 网格子项一律加 `min-w-0`（例如在容器上写 `[&>*]:min-w-0`），列表网格用 `grid-cols-1`，防止长仓库名把页面撑宽。

## 交付前检查（来自 skill）

- [ ] 不用 emoji 当图标，统一用 SVG
- [ ] 所有可点击元素有 `cursor-pointer` 和悬停状态（150–300ms）
- [ ] 文字对比度至少 4.5:1；焦点框清晰可见
- [ ] 尊重 `prefers-reduced-motion`
- [ ] 在 375、768、1024、1440 四个宽度检查过
- [ ] 点击区域至少 44×44px
