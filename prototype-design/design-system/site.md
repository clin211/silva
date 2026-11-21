# Silva 公开站点设计系统 v3：暗室

本版由 `design-taste-frontend` 抽取方向生成。方向是 **dark-room**（暗室），种子 2067718303，完整的方向表保存在 `design-system/direction.json`。实现文件是 `assets/silva.css`（令牌和组件）、`assets/site.js`（页头、页脚、动效、拖拽长廊）和 `assets/credits.js`（图片署名）。

## 核心想法

> 在这个站点上，**周屿像树一样一圈一圈的成长（Silva）**，变成**暗室里被照亮的年轮**。

Silva 不是「森林」的字面意思，而是像树一样走出舒适区，慢慢地、一圈一圈地往外长。站内每篇文章、每个项目都是新长出的一圈；旧的那圈不会被抹掉，这和后台「不可变修订」的设计一致。

**可交互的部分**：首页的「两次快照 / 一个增长」计算器。访客修改两天的 Stars 快照，右侧按 AI 榜单的真实口径即时重算：负增长照实显示，缺一天就写「数据不足」。

## 令牌

| 角色 | 值 | 说明 |
| ---- | -- | ---- |
| `--bg` | `#0f1f10` | 整页唯一的底色，不分色带 |
| `--surface` / `--surface-2` | `#162a17` / `#1d351e` | 靠色阶区分层次，不用描边和阴影 |
| `--ink` | `#f1f4ee` | 正文，偏暖的浅色，不用纯白 |
| `--muted` | `#b3bdb1` | 次要文字，在底色上的对比度约 9:1 |
| `--accent` | `#c6f19d` | 浅青柠。只用在页面唯一的主操作、小方块按钮和页脚字标上 |
| `--field-1` / `--field-2` | `#000` / `#fff` | 整块的黑场和白场，例如快照计算器 |
| 字体 | Boska + Noto Serif SC | 只用常规体和 500，不用粗体 |
| 字号 | 14 / 19 / 26 / 64 px，以及页脚字标 | 小字 / 正文 / 引导 / 标题 / 字标，一页不超过 5 级 |
| 圆角 | 0 | 所有元素都是方角 |

## 规则

这些规则用 `scripts/inspect.mjs` 实测：

- 标题在 1440、768、390 三个宽度下都不超过两行。
- 不用眉标、编号（01 / 02）、细分隔线、渐变、静止状态下的阴影、毛玻璃、图标卡片，也不用一排排同样的卡片。
- 等宽字体只用于真实代码和原样数据。需要豁免时，在同一行加 `taste-skill-ignore` 注释。
- 正文至少 16px；对比度至少 4.5:1。
- 每页至少 4 张真实图片，全部来自 `site/img/`（开放图库），并在页脚署名。代码绘制的 SVG 不计入。
- 相邻区块不用同一种布局，每一屏只有一个主体。
- 动效：区块内各部分依次入场（`data-rise`），图片缓缓归位（`data-zoom`），所有控件有按下、悬停和焦点反馈；首页有拖拽长廊，页脚是循环视频。开启「减少动态效果」时只保留内容，不做位移。

## 页面骨架

```html
<head> … <script>document.documentElement.classList.add('js')</script>
  Boska（fontshare）、Noto Serif SC（Google Fonts）、Tailwind CDN 3.4.17、../assets/tw-config.js、../assets/silva.css
<body class="silva" data-nav="…" data-proto-states="…">
  <div id="site-header"></div> <main id="main">…</main> <div id="site-footer"></div>
  <script src="../assets/credits.js"></script><script src="../assets/site.js"></script><script src="../assets/proto.js"></script>
```

## 检查方法

```bash
python3 -m http.server 8765 --directory prototype-design &
CHROME_PATH=<带 --no-sandbox 的 chrome 包装脚本> node ~/.claude/skills/design-taste-frontend/scripts/inspect.mjs \
  "http://localhost:8765/site/<页面>.html?bar=0" --out .taste-skill/inspect-<页面>
```

`?bar=0` 会隐藏原型工具条，避免被测成遮挡正文的元素。
