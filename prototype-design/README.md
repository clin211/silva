# Silva 前端原型

依据 `docs/04-frontend-design.md`（页面与布局）和 `docs/02-frontend-technical.md`（技术约束）绘制的一期 HTML 原型，用 HTML、Tailwind CSS（CDN 3.4.17）和原生 JS 写成。

打开 `index.html` 查看全部页面。直接双击就能打开，不需要构建，但要联网加载 Tailwind 和 Google Fonts。

## 目录

```text
prototype-design/
├── index.html          原型目录（W01–W09、A01–A21）
├── design-system/      公开站点设计系统（site.md）
├── assets/
│   ├── tw-config.js    设计令牌：颜色、字体、宽度
│   ├── base.css        长文排版、后台表格、状态标签、等高线动画
│   ├── silva.css       公开站点 v4 令牌、组件与长文排版
│   ├── site.css        原型目录页样式
│   ├── site.js         公开站点悬浮页头、页脚与入场动效
│   ├── admin.js        后台外壳：分组侧栏、面包屑、账户菜单、手机抽屉
│   └── proto.js        原型工具条、抽屉与弹窗、标签页、Toast
├── site/               公开站点，9 个页面
└── admin/              管理后台，21 个页面
```

## 原型工具条

每页左下角有一个工具条。它是看原型用的，不属于产品界面。

- **页面状态**：切换空态、请求失败、站点版本变化、构建失败、修订冲突等文档要求的状态。页面里加了 `data-state="…"` 的元素只在对应状态下显示。
- **当前角色**：只在后台页出现，可在 Author、Editor、Admin 之间切换。导航和按钮按 `data-role` 显示或隐藏。
- 状态和角色都能写进链接，例如 `admin/posts.html?role=author`、`site/ai-trending.html?state=nobatch`。

## 视觉方向

- 公开站点（v4「明亮门户」）：参考 clin.pro 的个人门户版式，详见 `design-system/site.md`。悬浮胶囊页头，青色主视觉，浅色、深色、黄色色带交替，双层圆角卡片，字体用 Geist。不用图库照片，文章封面是生成的色板。共享样式在 `assets/silva.css`，页头、页脚和动效在 `assets/site.js`。
- 管理后台：中性灰底，表格和表单排得紧凑。状态一律同时用文字和颜色表示。
- 首页和 About 的版式直接参考 clin.pro，文案换成 Silva 的内容。
