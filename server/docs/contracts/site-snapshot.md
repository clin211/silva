# 站点构建快照契约

来源：`docs/03-backend-technical.md` §7.2、`docs/02-frontend-technical.md` §3.1。

公开投影只包含访客可见的数据。文章草稿、账户、对象键与 Markdown 原文均保留在服务端。

## 1. 交付方式

Astro SSR 公开站点在请求时通过 `GET /api/v1/public/site-snapshot`（生产 Compose 可通过 `SILVA_SITE_SNAPSHOT_PATH` 指向内部路径）从服务端取得 `{ "data": … }`，不使用本地 mock 或示例内容。响应的 `X-Site-Generation` 对应本次读取的版本，空站点为 0。相同 ETag 的条件请求返回 304。订阅、页面、媒体和公开接口基于同一份已发布数据。

运行与构建环境变量：

| 变量 | 含义 |
| --- | --- |
| `SILVA_PUBLIC_API_ORIGIN` | Rust API 地址，本地默认 `http://127.0.0.1:8088` |
| `SILVA_SITE_SNAPSHOT_PATH` | 快照读取路径，默认 `/api/v1/public/site-snapshot` |
| `SILVA_SITE_OUT_DIR` | 构建输出目录，默认 `./dist` |
| `SITE_URL` | 公开站点的完整 origin，用于 canonical、RSS 和 sitemap |

页面在 `<head>` 中输出 `<meta name="silva:generation" content="<generation>">`，并按本契约校验返回结构；校验失败或上游不可用时返回 HTTP 503，不回退到 mock 或空数据。

## 2. 结构（schema_version 1）

时间均为带 `+08:00` 偏移的 RFC 3339 字符串，ID 为 UUIDv7。数组的顺序即展示顺序。

```json
{
  "schema_version": 1,
  "generation": 12,
  "generated_at": "2026-11-24T16:00:00+08:00",
  "posts": [
    {
      "id": "019535d9-3df7-79fb-b466-fa907fa17f9e",
      "revision_id": "019535da-0000-7000-8000-000000000001",
      "slug": "immutable-cms-revisions",
      "title": "用不可变 revision 重写 CMS 的发布流程",
      "excerpt": "把编辑版本、期望公开版本和当前线上版本分别记录。",
      "html": "<h2 id=\"三个版本指针\">三个版本指针</h2><p>…</p>",
      "headings": [{ "depth": 2, "slug": "三个版本指针", "text": "三个版本指针" }],
      "categories": ["backend"],
      "tags": ["cms", "postgresql"],
      "featured": true,
      "cover": {
        "media_id": "019535db-0000-7000-8000-000000000002",
        "mime_type": "image/png",
        "width": 1600,
        "height": 900,
        "alt": "封面说明"
      },
      "seo_title": null,
      "seo_description": null,
      "published_at": "2026-11-10T09:00:00+08:00",
      "updated_at": "2026-11-12T18:30:00+08:00"
    }
  ],
  "series": [],
  "categories": [{ "slug": "backend", "name": "后端", "description": "服务端架构与工程实践。" }],
  "tags": [{ "slug": "cms", "name": "CMS" }],
  "profile": null,
  "ai_trending": { "data_date": null, "repositories": [] },
  "media": [
    { "id": "019535db-0000-7000-8000-000000000002", "mime_type": "image/png", "width": 1600, "height": 900 }
  ]
}
```

### 2.1 posts

- 只含期望公开的文章，按 `published_at` 降序、`id` 升序排列。
- `excerpt` 可为 `null`；构建器可从正文文本截取摘要。
- `html` 是服务端在保存修订时由 Markdown 渲染并消毒的 HTML：不含脚本、事件属性、`style` 或原始 HTML；链接只允许 `http`、`https`、`mailto` 和站内相对地址。构建器原样插入，可在其上做代码高亮等不引入外部资源的增强。
- 正文图片只引用受控媒体：`<img src="/api/v1/public/media/{id}" width="…" height="…" alt="…" loading="lazy" decoding="async">`。
- `headings` 为 h2–h4，`slug` 与 `html` 中对应标题的 `id` 一致，用于目录。
- `categories`、`tags` 为 slug 列表，按名称排序，均能在顶层 `categories`、`tags` 中找到；文章没有分类时为空数组，第一个分类视为主分类。
- `cover` 为 `null` 时构建器使用默认封面样式；否则图片地址为 `/api/v1/public/media/{media_id}`。
- `published_at` 为文章首次被接受发布的时间；`updated_at` 为当前公开版本被接受发布的时间，可能等于 `published_at`。

### 2.2 categories / tags

只含至少被一篇已发布文章引用的条目。分类按名称排序，标签按名称排序。`description` 可为 `null`。

### 2.3 profile

个人资料尚未录入时为 `null`，构建器应输出不含个人信息的占位页面，而不是示例内容。录入后结构为：

```json
{
  "display_name": "长林啊",
  "headline": "全栈工程师",
  "bio_html": "<p>…</p>",
  "location": "上海",
  "avatar": null,
  "experiences": [
    {
      "organization": "Silva",
      "role": "独立开发者",
      "description_html": "<p>…</p>",
      "started_on": "2024-01-01",
      "ended_on": null
    }
  ],
  "skills": [{ "name": "Rust", "category": "后端", "proficiency": null }],
  "links": [{ "platform": "github", "label": "GitHub", "url": "https://github.com/example" }]
}
```

`avatar` 与 `cover` 结构相同。

### 2.4 可为 null 的字段

构建器必须接受下列字段为 `null`，不能因此让构建失败：

| 位置 | 可为 `null` 的字段 |
| --- | --- |
| 文章 | `excerpt`、`cover`、`seo_title`、`seo_description` |
| 图片（`cover`、`avatar`） | `alt` |
| 分类 | `description` |
| 个人资料 | `headline`、`bio_html`、`location`、`avatar` |
| 经历 | `description_html`、`started_on`、`ended_on` |
| 技能 | `category`、`proficiency`（文本，如「熟练」） |
| 链接 | `label`（缺省时显示 `platform`） |

链接的 `url` 由后台录入，服务端不限制协议；构建器只渲染 `http`、`https` 与 `mailto` 链接，其他协议的链接不输出。

### 2.5 ai_trending

AI 采集尚未实现。快照固定为 `{"data_date": null, "repositories": []}`，构建器显示「暂无数据」。采集实现后在此扩展字段，并递增 `schema_version`。

### 2.6 media

快照引用的全部媒体（文章和系列封面、正文图片、头像）。Public API 的 `GET /api/v1/public/media/{id}` 只提供当前线上快照中列出的媒体，其他 ID 返回 404。

### 2.7 series

公开 API 在 v1 顶层输出 `series` 数组，旧快照缺省时按空数组读取。显式 null、无效封面、悬空或重复文章、空篇章或空系列均使校验失败。

每项包含 `id`、`slug`、`title`、`description`、`category`、`completion_status`、`featured`、`cover`、`seo_title`、`seo_description`、`published_at`、`updated_at`、`items` 和 `sections`。可空的 category 内含 slug、name 和 description，可空的 cover 使用与文章相同的图片结构。公开时间取系列首发受理时间，更新时间取系列与可见文章更新时间中的最大值。

系列按排序权重升序、首次公开时间降序、ID 升序排列。阅读顺序先取根级 items，再逐章展开 sections。篇章包含 title、description 和 items。条目包含 post_slug 和 display_title，未设短标题时导出本代文章标题。只有具有期望公开修订的成员参与导出。系列封面按 ID 并入 media；系列主分类自带详情，顶层 categories 仍统计文章分类。后台编辑版本用于并发校验。

## 3. 线上读取

Astro 为每个页面请求确定一个公开版本，用该版本生成详情、归档或订阅。详情不存在时返回 HTTP 404；读取失败时返回 503。部署包内的 CSS、字体和脚本由 Node 提供。

## 4. 版本演进

新增可选字段不改变 `schema_version`；删除字段、改变字段含义或类型时递增 `schema_version`，并同时更新 Rust 投影与 Astro 校验。
