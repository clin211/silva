# 站点构建快照契约

来源：`docs/03-backend-technical.md` §7.2、`docs/02-frontend-technical.md` §3.1。

Worker 为每个站点 `generation` 导出一份不可变的 JSON 快照。Astro 只从这份快照构建公开站点，Public API 也只读取当前线上 generation 的同一份快照，两者内容一致。快照只含公开投影：不含账户、草稿、Markdown 原文、对象存储键或任何内部字段。

## 1. 交付方式

Worker 把快照写入临时文件，再在 `frontend/apps/website` 中执行 Astro 构建，通过环境变量传入：

| 变量 | 含义 |
| --- | --- |
| `SILVA_SNAPSHOT` | 快照文件的绝对路径。设置后进入快照模式；未设置时使用仓库内的示例内容（仅供本地开发） |
| `SILVA_SNAPSHOT_SHA256` | 快照文件字节的 SHA-256（小写十六进制） |
| `SILVA_SNAPSHOT_GENERATION` | 期望的 generation，十进制整数 |
| `SILVA_SITE_OUT_DIR` | 构建输出目录的绝对路径，默认 `./dist` |
| `SITE_URL` | 公开站点的完整 origin，用于 canonical、RSS 和 sitemap |

快照模式下，构建必须在读取任何页面数据之前完成三项校验，任一失败即以非零状态退出，不回退到示例内容或空数据：

1. 文件字节的 SHA-256 等于 `SILVA_SNAPSHOT_SHA256`。
2. `schema_version` 等于构建器支持的版本（当前为 `1`）。
3. `generation` 等于 `SILVA_SNAPSHOT_GENERATION`。

构建产物的每个 HTML 页面在 `<head>` 中输出 `<meta name="silva:generation" content="<generation>">`。

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

快照引用的全部媒体（封面、正文图片、头像）。Public API 的 `GET /api/v1/public/media/{id}` 只提供当前线上快照中列出的媒体，其他 ID 返回 404。

## 3. 产物与清单

Worker 把构建输出上传到 `site/{generation}/{attempt}/dist/`，快照上传到 `site/{generation}/{attempt}/snapshot.json`，并写入 `site/{generation}/{attempt}/manifest.json`：

```json
{
  "generation": 12,
  "attempt": 1,
  "snapshot_sha256": "…",
  "files": [{ "path": "index.html", "size": 10240, "sha256": "…", "content_type": "text/html; charset=utf-8" }]
}
```

线上切换只在全部文件和清单上传成功后进行。公开入口按 `site_state.active_generation` 对应任务的 `artifact_prefix` 读取文件；清单中不存在的路径返回 404 页面，不读取其他 generation 的产物。

## 4. 版本演进

新增可选字段不改变 `schema_version`；删除字段、改变字段含义或类型时递增 `schema_version`，并同时更新 Worker 导出与构建器校验。
