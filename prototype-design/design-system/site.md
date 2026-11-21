# Silva 公开站点设计系统 v4：明亮门户

参考站主之前的个人站 [clin.pro](https://clin.pro/)。Silva 是一个以博客为主体的个人门户，不是摄影作品集，所以不用图库照片。站点名的意思是像树一样一圈一圈地成长，不是字面上的森林，因此视觉上不出现森林、树木或年轮的图片。

实现文件是 `assets/silva.css`（令牌和组件）和 `assets/site.js`（悬浮页头、四栏页脚、入场动效、图标）。

## 页面结构

- **页头**：顶部悬浮的胶囊导航，白底 88% 透明度加毛玻璃。左边是字标，中间放栏目，右边是 RSS 按钮。手机上改为下拉面板。
- **首页**依次是：青色主视觉（姓名、标语、行动按钮，加一张显示站点实时数据的卡片）→ 最近更新（一篇头条文章配三篇列表）→ 正在发生（深色区，包括精选项目、AI 开源趋势和正在输入）→ 关于（浅灰绿区，左边是介绍，右边是工作方法）→ 保持联系（黄色区）→ 页脚。
- **About**：上方左边是标题、右边是自我介绍和四个关注方向。下方左栏是个人档案卡片（头像字、所在地、工作经验、邮箱和 GitHub 语言概览），可以吸顶；右栏依次是个人简介、工作经历、技术能力。再往下是「一些自叙」四张彩色卡片和结尾的行动按钮。
- **其他页面**统一用 About 页头的写法（小标签、大标题、引导语），列表用双层卡片或分隔行。

## 令牌

| 角色 | 值 | 用途 |
| ---- | -- | ---- |
| `--bg` | `#F4F8F7` | 页面底色 |
| `--paper` | `#FFFFFF` | 卡片表面、页脚 |
| `--mist` | `#EDF2F0` | 交替色带 |
| `--ink` / `--ink-2` / `--muted` | `#102523` / `#33484A` / `#5B6F6C` | 正文、次要文字、辅助文字 |
| `--teal` / `--accent` | `#087F89` / `#0B8992` | 主视觉色带；链接、悬停、焦点 |
| `--deep` | `#071A18` | 深色区（正在发生） |
| `--sun` | `#F6BF35` | 联系区、强调标签 |
| 字体 | Geist、Geist Mono，中文用系统黑体 | 标题 600，正文 400 |
| 圆角 | 胶囊按钮和标签用 999px，卡片外层 2rem、内层 2rem − 6px，面板 1.25rem | |

## 组件

- `.band` 加 `.band-teal`、`.band-mist`、`.band-deep`、`.band-sun`、`.band-paper`：整块色带区。
- `.eyebrow`：带圆点的小胶囊标签，用来命名区块。`.kicker`：英文小标题，如 Build log。
- `.soft`：双色标题的第二行，用浅一些的颜色。
- `.bezel > .face`：双层卡片，外层是淡色壳，内层是白色面；放在深色区时自动换成深色。
- `.panel`：单层白卡片。`.chip`（以及 `-teal`、`-sun`、`-coral`、`.hash`）：标签和状态。
- `.btn` 加 `-dark`、`-light`、`-ghost`、`-sm`：胶囊按钮，可以内嵌 `.dot` 箭头。
- `.cover` 加 `c-teal`、`c-deep`、`c-sun`、`c-coral`、`c-ink`、`c-mist`：文章封面不用照片，而是一块纯色色板，加点阵纹理和等宽字标记（如 `rev:v2`、`pg`、`fn`）。
- `.choice`：分段胶囊选择器。`.input`、`select.select`：输入框和下拉框。`.data-table`、`.bar`、`.prose`：表格、进度条、长文排版。
- 动效：`data-rise` 让区块内的子元素依次淡入、上移、去模糊；首页主视觉用 `.hero-reveal`。开启「减少动态效果」时全部关闭。

## 页面骨架

```html
<head> … <script>document.documentElement.classList.add('js')</script>
  Geist / Geist Mono（Google Fonts）、Tailwind CDN 3.4.17、../assets/tw-config.js、../assets/silva.css
<body class="silva" data-nav="…" data-proto-states="…">
  <div id="site-header"></div> <main id="main">…</main> <div id="site-footer"></div>
  <script src="../assets/site.js"></script><script src="../assets/proto.js"></script>
```

页头是悬浮的，所以 `main` 默认留出 96px 的上边距。首页主视觉要延伸到页头下面，给 `main` 加 `class="flush"`。
