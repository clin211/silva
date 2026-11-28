# Developer Hub 文章系列（Series）设计文档

- 版本：1.1
- 状态：方式 A 已确认，以下修订为实施与验收方案，待开发
- 适用模块：PostgreSQL Schema、Rust API、Astro Website、React Admin、文章批量导入工具
- 相关文档：[产品文档](01-product.md)、[前端技术文档](02-frontend-technical.md)、[后端技术文档](03-backend-technical.md)、[前端页面设计](04-frontend-design.md)、[数据库契约](../server/docs/contracts/database.md)、[RESTful 契约](../server/docs/contracts/restful-api.md)、[站点快照契约](../server/docs/contracts/site-snapshot.md)

---

## 1. 背景与产品定位

当博客持续输出成体系的技术内容（例如 Rust 体系下的《Rust 基础》《Rust 进阶》《Rust 并发编程》《Rust Web》，以及现有 `Full-Stack-Notes` 中的 Go、React、Next.js、Makefile、算法等连续笔记）时，仅靠无序的分类和标签无法表达阅读顺序、章节结构与学习进度。

文章系列（Series / 专栏）用于在现有文章管理模块之上提供**有序的目录编排与连载阅读体验**。

### 1.1 分类、标签与系列的职责边界

| 维度 | 职责定位 | 是否有序 | 典型示例 |
| --- | --- | --- | --- |
| **分类（Category）** | 横向学科与技术领域划分，也是多系列的聚合维度 | 无序 | `Rust`、`Go`、`Web 前端与全栈` |
| **标签（Tag）** | 细粒度主题与知识点，支持跨分类横向关联 | 无序 | `Tokio`、`Axum`、`并发编程`、`内存管理` |
| **系列（Series）** | 纵向成体系的连载教程或在线小册，具备封面、导语、连载状态与严格目录顺序 | **强有序** | 《Rust 基础入门》《Rust 并发编程》《Rust Web 实战》 |

### 1.2 组织模型（已定稿：方式 A）

采用 **「主分类聚合多独立系列 + 系列内可选篇章分组」** 模型：

```text
分类：Rust
├── 系列：Rust 基础入门（18 篇）
│   ├── 篇章：一、环境与核心语法（6 篇）
│   ├── 篇章：二、所有权与生命周期（6 篇）
│   └── 篇章：三、泛型、Trait 与错误处理（6 篇）
├── 系列：Rust 进阶（12 篇，可直接扁平挂文章或按篇章拆分）
├── 系列：Rust 并发编程（15 篇）
└── 系列：Rust Web 开发（20 篇）
```

- 每个专题独立建为一个 `series`，并通过可选的 `category_id` 归入同一个主分类（如 `Rust`），在 `/series/` 总览页与 `/categories/rust/` 分类页自然聚合展示。
- 系列内部支持可选的 `series_sections`（篇章分组）：短系列（3–10 篇）无需建篇章，直接扁平挂载文章；长系列（十几至数十篇）可按篇章划分，保持目录清晰。

### 1.3 一期范围与非目标

| 范围 | 一期必做 | 后续扩展（一期不做） |
| --- | --- | --- |
| 数据与编排 | 系列 CRUD、可选篇章分组、有序目录、目录短标题、主分类关联、连载中/已完结状态、公开/下线发布 | 一篇文章归属多个系列、定时公开系列、系列级修订历史/审核 |
| 公开站点 | `/series/` 总览页、`/series/[slug]/` 专栏页、`/blog/[slug]/` 系列目录与系列内上下篇、分类页与文章列表联动 | 系列独立 RSS、跨系列学习路线图、访客云端阅读进度同步 |
| 管理后台 | 系列列表页、系列工作台（基础信息 + 目录编排 + 快捷新建文章）、文章列表与编辑器联动 | 复杂权限细分（如 Author 独立管理自有系列） |
| 草稿处理 | 后台可将草稿提前排入目录；**公开站点严格只展示已发布文章，未发布草稿不展示** | 前台「敬请期待」灰色占位展示 |

---

## 2. 核心架构决策

1. **目录编排挂载在 `posts.id`，不挂载在 `post_revisions.id`**
   - 系列目录独立保存归属和排序。调整顺序只需更新目录，文章正文沿用各自的发布目标。
   - 因此职责分离：`post_revisions` 只管单篇文章正文内容演化；`series` / `series_sections` / `series_items` 独立管理目录编排。
2. **文章 URL 保持扁平 `/blog/{post_slug}/` 不变**
   - Silva 文章首次发布后锁定 `slug`（`slug_locked = true`）。文章 URL 不嵌套系列 `slug`，避免文章移入/移出系列或跨系列调整时产生断链与 SEO 损失。
   - 系列自身使用独立路由 `/series/` 与 `/series/{series_slug}/`。
3. **草稿自动过滤与空系列隐藏**
   - 后台目录可包含草稿、已发布等任意状态的文章，方便作者先规划整套大纲再逐篇写作。
   - Public API 读取公开数据时，仅包含已公开系列（`published_at IS NOT NULL`）中已发布文章（`EXISTS (SELECT FROM published_posts WHERE post_id = posts.id)`）。
   - 过滤后为空的篇章自动省略；过滤后没有任何已发布文章的系列不导出到公开快照。
4. **独立关联表 + 一期单系列归属约束**
   - 使用独立表 `series_items` 存储系列与文章映射，并对 `post_id` 施加 `UNIQUE` 约束，保证文章页的面包屑、系列目录和上下篇导航唯一无歧义。
   - 未来如需支持「一篇文章属于多个专题」，需同时调整唯一约束、文章 API 的单值归属及文章页的导航上下文选择。
5. **公开状态与连载状态正交拆分**
   - `published_at` 表示是否对外公开（`NULL` 为未公开，非空为设为公开）；`completion_status` 表示内容连载状态（`ongoing` 连载中 / `completed` 已完结）。
   - 允许「连载中即对外公开」，也允许「已完结但暂不公开或后续下线」。

---

## 3. 数据库设计

新增 SQLx 迁移文件 `server/migrations/<timestamp>_series.sql`，严格遵守 [数据库契约](../server/docs/contracts/database.md)（UUIDv7 主键校验、显式 `ON DELETE`、全量 `COMMENT ON TABLE/COLUMN`、`updated_at` 触发器）。

### 3.1 新增数据表

```sql
CREATE TABLE series (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  slug text NOT NULL UNIQUE
    CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 100),
  title text NOT NULL,
  description text,
  cover_media_id uuid REFERENCES media_assets(id) ON DELETE RESTRICT,
  category_id uuid REFERENCES categories(id) ON DELETE RESTRICT,
  completion_status text NOT NULL DEFAULT 'ongoing'
    CHECK (completion_status IN ('ongoing', 'completed')),
  is_featured boolean NOT NULL DEFAULT false,
  sort_order integer NOT NULL DEFAULT 0,
  seo_title text,
  seo_description text,
  published_at timestamptz,
  first_published_at timestamptz,
  edit_version bigint NOT NULL DEFAULT 0 CHECK (edit_version >= 0),
  publication_version bigint NOT NULL DEFAULT 0 CHECK (publication_version >= 0),
  created_by uuid REFERENCES users(id) ON DELETE NO ACTION,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK (published_at IS NULL OR first_published_at IS NOT NULL)
);

CREATE TABLE series_sections (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  series_id uuid NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  title text NOT NULL,
  description text,
  position integer NOT NULL CHECK (position >= 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (series_id, id),
  UNIQUE (series_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE series_items (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  series_id uuid NOT NULL REFERENCES series(id) ON DELETE CASCADE,
  section_id uuid,
  post_id uuid NOT NULL UNIQUE REFERENCES posts(id) ON DELETE CASCADE,
  position integer NOT NULL CHECK (position >= 0),
  display_title text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE NULLS NOT DISTINCT (series_id, section_id, position) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (series_id, section_id)
    REFERENCES series_sections(series_id, id) ON DELETE NO ACTION
);

```

复合外键保证条目和篇章属于同一系列。删除篇章时按最终目录先移动或移除条目，再删除空篇章。移到根目录只更新 `section_id` 为 NULL，保留非空的 `series_id`。删除整个系列级联删除编排记录，保留文章与修订。

同步发布迁移 `20261127000001` 移除原有的 `site_generation_series`。API 依据系列的公开设置和可见成员计算线上状态。

### 3.2 索引、触发器与视图更新

- **触发器**：为 `series`、`series_sections`、`series_items` 统一挂载 `BEFORE UPDATE ... EXECUTE FUNCTION maintain_updated_at()`。
- **查询索引**：
  - `CREATE INDEX series_category_sort_idx ON series (category_id, sort_order ASC, created_at DESC);`
  - `CREATE INDEX series_desired_published_idx ON series (published_at DESC) WHERE published_at IS NOT NULL;`
  - `CREATE INDEX series_cover_media_idx ON series (cover_media_id) WHERE cover_media_id IS NOT NULL;`
  - PostgreSQL 17 的两个可延迟唯一约束自带排序索引；根目录的 NULL 也参与唯一性校验。事务中可交换位置，提交时每组内的序号必须唯一。
- **媒体引用视图（`media_references`）**：
  - 引用视图以 `series_id` 标识 `series.cover_media_id` 的使用位置（`kind = series_cover`）。
  - 写入封面时在事务内检查媒体访问权限与 `ready` 状态。引用总数包含全部来源，详情按 `kind` 分流：系列位置仅向有 `series.manage` 的用户显示。同步更新媒体详情、删除冲突 `media_usage`、OpenAPI 的引用枚举及后台引用列表。
- **不变量验证脚本**：
  - 同步更新 `server/tests/sql/schema_invariants.sql`，纳入 3 张实体表的 UUIDv7 主键、复合外键，以及 3 张表的非空注释、触发器适用范围与外键删除策略检查。

---

## 4. 权限、领域规则与发布触发

### 4.1 权限定义

| 权限键 | 说明 | 授予角色 |
| --- | --- | --- |
| `series.manage` | 创建、编辑、删除系列及编排目录大纲 | `editor`、`admin` |
| `posts.publish`（复用） | 公开或取消公开系列 | `editor`、`admin` |

系列管理端的读取、筛选与所有写入均校验 `series.manage`；公开与下线额外校验 `posts.publish`。将新权限加入 `domain::identity::EDITOR_PERMISSIONS`，通过现有角色初始化逻辑补齐存量角色授权。Author 保留原有文章读取范围，文章响应中的系列摘要仅向有系列管理权限的用户提供。同步更新 OpenAPI 的权限定义和后台 `PERMISSIONS`，前端按相同权限控制路由和按钮。

### 4.2 字段校验规则（`domain::series`）

| 字段 | 规则 |
| --- | --- |
| `slug` | 复用 `validate_slug`：`^[a-z0-9]+(-[a-z0-9]+)*$`，1–100 字节；`slug_locked = true` 后禁止修改 |
| `title` | 去首尾空白，1–100 字符，不含控制字符 |
| `description` | 可选，空白视为 `None`，1–1000 字符 |
| `completion_status` | 枚举：`ongoing` / `completed` |
| `seo_title` / `seo_description` | 与文章修订一致：分别最多 200 / 500 字符 |
| `section.title` | 1–100 字符；`section.description` 可选，最多 500 字符 |
| `item.display_title` | 可选，空白视为 `None`，1–200 字符 |
| 容量上限 | 单系列最多 50 个篇章、最多 200 篇文章；同一请求内 `post_id` 不得重复 |

### 4.3 发布触发与事务边界

下表的“设为公开”指 `published_at IS NOT NULL`。“在线”要求系列已设为公开且至少含一篇已发布文章。公开允许目录暂时为空，首次接受即锁定 slug；首次有成员文章公开时专栏页即可访问。

| 操作 | 成功结果 | 公开版本更新条件 |
| --- | --- | --- |
| 创建系列草稿 | `201`，两个版本号从 0 开始 | 无 |
| 修改元数据、替换目录、单篇加入或移出 | 元数据与目录返回 `200`，加入返回 `201`，移出返回 `204` | 设为公开且发生实际变更时，同事务更新一次公开版本（`series.update`） |
| 公开 | `200`；写入公开时间，首次写入 `first_published_at` 后锁定 slug，发布版本加一 | 一次 `series.publish` |
| 下线 | `200`；清空公开时间，保留首发时间与 slug 锁，发布版本加一 | 一次 `series.unpublish` |
| 删除系列 | 仅当未设为公开时返回 `204`；否则 `409 series_in_use` | 无。已公开的系列应先取消公开 |
| 成员文章发布、换版或下线 | 复用文章发布流程 | 随后读取目录、计数与邻接关系时可见新数据 |
| 修改系列主分类的名称或描述 | 复用分类更新流程 | 现有分类查询增加“被可导出系列引用”的判断；同一次分类修改最多递增一次公开版本。删除仍被任一系列引用的分类返回 `409 taxonomy_in_use` |

写操作在同一数据库事务锁定系列行，校验版本后更新数据及审计。公开内容变化时调用 `advance_generation`，通过站点状态行锁分配新的 generation 并立即生效。任一步失败整体回滚。元数据、全量目录、单条加入或移出共用 `edit_version`。实际变更将版本加一并更新系列时间，无变化保存返回当前资源。版本过期返回 `409 series_edit_conflict`。删除同时检查编辑版本、发布版本和公开状态。

发布转换单独校验 `expected_publication_version`。过期返回 `409 publication_conflict`；版本匹配时重复公开返回 `409 already_published`，重复下线返回 `409 already_unpublished`，均不增加公开版本。发布或下线成功后返回 200，响应中的版本已经生效。

---

## 5. RESTful API 契约

以下均为带认证的后台接口，路径以实际 Axum 路由和 [OpenAPI](../server/apps/api/openapi.json) 使用的 `/api/v1/admin` 为前缀。Astro 按请求通过 `GET /api/v1/public/site-snapshot` 读取公开系列与文章。

### 5.1 系列接口

| 方法与路径（相对于后台前缀） | 请求与用途 | 成功响应 |
| --- | --- | --- |
| `GET /series` | `q`（标题/slug）、`category_id`、`published`（期望状态）、`completion_status`、`sort`（默认 `sort_order_asc`，另有 `updated_desc` / `created_desc`）、`page`、`per_page`；同值以 id 升序 | `200`，列表与 `meta.pagination`，默认每页 20，最多 100 |
| `POST /series` | 必填标题与 slug，其余元数据取表定义默认值 | `201`，系列与 `Location: /api/v1/admin/series/{id}` |
| `GET /series/{id}` | 元数据、两种版本、线上状态、统计和完整目录；条目含文章编辑标题、期望/线上修订指针 | `200` |
| `PATCH /series/{id}` | `expected_edit_version` 与要修改的字段；省略即保留，可空字段显式传 null 即清除 | `200`，返回更新后的系列及可空的公开版本记录 |
| `DELETE /series/{id}` | 查询参数 `expected_edit_version`、`expected_publication_version`，适用 §4.3 删除条件 | `204` |
| `PUT /series/{id}/publication` | JSON 必填 `expected_publication_version` | `200`，复用 `PublicationResult` |
| `DELETE /series/{id}/publication` | 查询参数必填 `expected_publication_version`，无请求体 | `200`，同上 |
| `PUT /series/{id}/outline` | `expected_edit_version` 与完整目录；返回分配的篇章 ID | `200`，更新后的系列及可空的公开版本记录 |
| `POST /series/{id}/items` | `expected_edit_version`、`post_id`，可选 `section_id` 和 `display_title`；追加至组末尾 | `201`，条目、最新编辑版本、可空的公开版本记录，Location 指向下行资源 |
| `DELETE /series/{id}/items/{post_id}` | 查询参数必填 `expected_edit_version`；仅移除本系列中的成员 | `204`，客户端刷新系列和文章详情 |

沿用成功信封 `data` 与现有错误信封。`200` 写响应的 `data` 为 `{series, publication}`，其中 `publication` 为 `null` 或 `{generation}`；发布转换的 `PublicationResult` 另含 `publication_version`。不存在的目标资源返回 404，重复输入与无效的篇章归属按字段返回 422，跨系列抢占或重复追加返回 `409 post_already_in_series`。记录冲突时客户端回读目录核对占用项。slug 重复或已锁定复用 `slug_in_use` / `slug_locked`。新增 409 错误码同步登记共享错误类型、HTTP 映射、响应契约与 OpenAPI。

### 5.2 `PUT /api/v1/admin/series/{id}/outline` 请求结构示例

```json
{
  "expected_edit_version": 7,
  "sections": [
    {
      "id": "019535e1-0000-7000-8000-000000000001",
      "title": "一、基础并发原语",
      "description": "线程、消息传递与共享状态",
      "items": [
        { "post_id": "019535d9-0000-7000-8000-000000000001", "display_title": "线程与作用域线程" },
        { "post_id": "019535da-0000-7000-8000-000000000002", "display_title": null }
      ]
    }
  ],
  "items": [
    { "post_id": "019535d8-0000-7000-8000-000000000003", "display_title": "开篇导读" }
  ]
}
```

- `sections[].id` 可为已有 UUIDv7（保留并更新）或省略/`null`（由服务端分配新 UUIDv7）；未出现在请求中的旧篇章将被删除。
- `sections` 与根级 `items` 均必填，空数组表示清空该层。请求中每个既有篇章 ID 必须唯一且属于当前系列；所有层的 `post_id` 合计去重，验证文章存在、单系列归属及 §4.2 容量。全量提交中遗漏的条目解除关联。
- 锁定父行后比较版本，先校验完整目标，再按最终数组写入。不存在的文章返回 404。先处理旧条目再删废弃篇章；任一冲突回滚整棵目录。并发将同一文章加入不同系列时，以 `UNIQUE(post_id)` 仲裁并映射到同一业务 409。增删单条也走同一把父行锁与容量校验。
- 根条目、篇章、篇章内条目分别从 0 编号。组内按 `position, id` 读，追加取组内最大位置加一（空组为 0）。公开阅读顺序固定为根条目在前，随后按篇章逐组展开。过滤草稿后重新从 1 编阅读序号。
- 仅目录 PUT 复用文章内容路由的 1 MiB 请求上限，避免 200 个多字节短标题和 50 个篇章被全局 64 KiB 提前拒绝；超出字节上限返回 413，超过条数或字段长度返回 422。

### 5.3 现有文章接口兼容增强

- `GET /api/v1/admin/posts`：新增查询参数 `series_id=<uuid>` 与 `in_series=true|false`，仅系列管理者可用；`series_id` 与 `in_series=false` 同传返回 422，筛选在分页、计数之前应用。
- 文章列表侧栏的 `GET /api/v1/admin/posts/category-counts` 同步接受上述系列条件，继续限制在当前用户与所选作者的可见范围内。筛选写入列表 URL，切换时回到第一页，查询缓存键包含系列条件。
- `PostSummary` 与 `Post` 响应新增可选只读字段：
  ```json
  {
    "series": {
      "id": "019535e0-0000-7000-8000-000000000004",
      "slug": "rust-concurrency",
      "title": "Rust 并发编程",
      "section_title": "一、基础并发原语",
      "position": 1
    }
  }
  ```
  未归属或无系列管理权限时为 `null`。`position` 为后台组内从 0 起的位置。编辑器通过系列详情获取最新 `edit_version` 后再操作。

---

## 6. 公开数据快照契约扩展

沿用 [v1 快照契约](../server/docs/contracts/site-snapshot.md)。在顶层增加可选数组 `series`。Rust 的 `SiteSnapshot` 反序列化使用 `#[serde(default)]`，Astro 的 Zod schema 对缺省值取 `[]`。当前 API 总是输出数组；显式 null 或结构损坏仍视为错误。下例是**叠加到契约 §2 完整示例上的新增字段**，引用该示例已列出的文章和媒体：

```json
{
  "series": [
    {
      "id": "019535e0-0000-7000-8000-000000000001",
      "slug": "cms-publishing",
      "title": "CMS 发布实践",
      "description": "从修订到站点发布。",
      "category": { "slug": "backend", "name": "后端", "description": "服务端架构与工程实践。" },
      "completion_status": "ongoing",
      "featured": true,
      "cover": {
        "media_id": "019535db-0000-7000-8000-000000000002",
        "mime_type": "image/png",
        "width": 1600,
        "height": 900,
        "alt": null
      },
      "seo_title": null,
      "seo_description": null,
      "published_at": "2026-11-20T10:00:00+08:00",
      "updated_at": "2026-11-22T14:00:00+08:00",
      "items": [],
      "sections": [
        {
          "title": "发布基础",
          "description": null,
          "items": [
            { "post_slug": "immutable-cms-revisions", "display_title": "修订与发布" }
          ]
        }
      ]
    }
  ]
}
```

### 6.1 公开读取与兼容性

1. API 在 `REPEATABLE READ READ ONLY` 事务中读取当前 generation、系列、篇章、成员与文章公开副本。顶层系列按 `sort_order ASC, first_published_at DESC, id ASC` 排列；只保留能关联到本次 `posts` 的条目、非空篇章和非空系列。回退标题取该快照的文章标题。系列的 `published_at` 取首发受理时间，`updated_at` 取系列更新时间与可见文章更新时间的最大值。
2. v1 的顶层 `categories` 仍只含有公开文章的分类。系列的可空 `category` 自带同一分类行的 slug、名称、简介，允许“系列属于 Rust，其中的文章都属于其他分类”。分类归档路由取文章分类与可见系列分类的并集，按 slug 去重；只有系列时也生成归档页。分类选择器使用同一份数据，文章数与系列数分别统计。
3. 把可见系列封面并入顶层 `media`，按媒体 ID 去重，沿用公开媒体地址。Astro `snapshot.ts` 校验系列 ID/slug 唯一、文章 slug 存在且全目录只出现一次、篇章/系列非空、封面在媒体清单中。SSR 无法取得有效公开快照时返回 503。离线夹具构建同样校验输入。
4. 系列封面仅在系列可见时出现在公开媒体投影，后台现用封面始终计入删除保护。
5. Astro 的 `data.ts` 按 §5.2 的顺序生成一次 `Map<postSlug, PostSeriesContext>`，用于全站序号、总数与跨篇章上下篇。精选开关仅在总览卡片标注「精选」，保持既定顺序；SEO 标题和描述缺省时回退到系列标题、简介，无封面时用站点默认样式。预计阅读时长累加这些公开文章的阅读分钟数；首篇无上一篇，末篇无下一篇，单篇显示返回目录。未入选文章按原规则导航。系列页使用自身 canonical；只将本代可见详情加入 sitemap。下线后的详情走站点真实 404，文章 URL 与现有文章 RSS 继续独立工作。

---

## 7. 公开站点页面设计（`frontend/apps/website`）

延续 [前端页面设计](04-frontend-design.md) 的视觉语言与响应式规则：

| 路由 | 页面 / 改动 | 核心内容与交互 |
| --- | --- | --- |
| 全局导航 | `Header.astro` / `site.ts` | 导航更新为 `首页 / 文章 / 系列 / AI 趋势 / 关于`，页脚同步补充「系列」入口 |
| `/series/` | 系列总览页（新增） | 页头简介 → **按主分类筛选条**（如 `全部 / Rust / Go / Web 前端与全栈`）→ 无可见系列时显示空状态；有内容时显示双列/三列专栏卡片（封面、标题、简介、连载中/已完结徽章、已发布篇数、总预计阅读时长） |
| `/series/[slug]/` | 系列专栏主页（新增） | 面包屑（`首页 / 系列 / 分类 / 系列名`，无主分类时省略分类）→ 专栏头部（封面、标题、连载状态、简介、统计「共 N 篇 · 约 M 分钟 · 最近更新」、主按钮「从第 1 篇开始阅读」）→ 完整有序目录（扁平编号或分章展示，`display_title ?? post.title`）→ 底部「同分类其他系列」推荐区（如《Rust 基础》页底展示《Rust 进阶》《Rust 并发编程》《Rust Web》） |
| `/blog/[slug]/` | 文章详情页（增强） | 若文章属于系列：① 标题上方/下方展示紧凑系列条（`系列：《Rust 并发编程》· 第 3 / 15 篇`，可跳转专栏主页）；② 右侧 `reading-rail` 增加「系列目录」区块（与现有 `ArticleToc` 并存或 Tab 切换，高亮当前篇；移动端置于正文前折叠区）；③ 底部 `article-neighbors` 改为**系列内上一篇 / 下一篇**，末篇附「返回系列目录」入口。非系列文章保持现状 |
| `/categories/[slug]/` | 分类归档页（增强） | 当该分类下存在已发布系列时，顶部优先渲染「相关系列」紧凑卡片组，下方接原有按月文章归档 |
| `/blog/` | 文章列表页（增强） | 属于系列的文章在元信息区附带小型系列角标（如 `Rust 基础 · 03`），点击可跳转专栏页 |

---

## 8. 管理后台页面设计（`frontend/apps/admin`）

在 `admin-shell.tsx` 的「内容管理」分组下新增 **系列**（`/series`），位于「文章」与「分类与标签」之间（权限要求 `series.manage`）：

### 8.1 系列列表页（`/series`）

- 顶部操作栏：搜索框（标题 / slug）、主分类筛选、公开状态筛选、连载状态筛选、「新建系列」按钮。
- 列表展示：封面缩略图、标题与 slug、所属主分类、连载状态徽章（连载中 / 已完结）、公开意向与线上状态、文章进度（`已发布 12 / 目录共 15 篇`）、排序权重、更新时间与操作入口。

- 列表和详情返回 `published_at`、`is_live`、`edit_version`、`publication_version`、`item_count` 和 `published_post_count`。后者按目录中具有已发布修订的文章计数。服务端根据公开设置与当前可见成员计算是否在线。
- 未设置公开的系列显示「未公开」。已设为公开但没有已发布成员的系列显示「暂无可公开文章」，添加或发布第一篇成员文章后显示「已上线」。编辑和发布失败时保留本地输入，回读服务端已提交的数据核对状态。

### 8.2 系列工作台（`/series/$seriesId`）

采用双区布局（桌面左右或上下分栏，窄屏纵向排列）：

1. **基础信息与发布区**
   - 标题、slug（未锁定前可编辑，锁定后只读提示）、主分类选择、封面图选择（复用 `MediaPickerDrawer`）、简介、连载状态切换（连载中 / 已完结）、精选开关、排序权重、SEO 标题与描述。
   - 顶部操作按钮：保存基础信息、公开系列 / 取消公开、删除系列（期望未公开且已不在线时可用）。
2. **目录大纲编排区**
   - 工具栏：「新增篇章分组」、「添加已有文章」、「在此系列新建文章」、「保存目录」。
   - **添加已有文章**：弹窗搜索尚未归属任何系列的文章，显示文章标题与发布状态（草稿/已发布），支持多选加入指定篇章或根目录。
   - **在此系列新建文章**：收集标题和可编辑的合法 slug，通过现有 `POST /api/v1/admin/posts` 创建草稿，传 `markdown_body: ""`，把系列主分类作为初始 `category_ids`。之后用返回的文章 ID 调用单条加入接口。入口同时要求 `posts.create` 和 `series.manage`。有未保存目录时先保存成功，以取得真实篇章 ID 和最新版本。创建成功、加入失败时保留文章 ID 和编辑入口，刷新目标篇章后只重试加入；响应丢失时先用 slug 或成员详情核对结果。
   - **目录条目交互**：支持拖拽排序（或上移/下移按钮）及跨篇章移动、行内编辑可选的 `display_title`、展示文章当前发布状态、一键跳转文章编辑页、一键移出系列。
   - 目录变更先在前端本地缓冲，显式点击「保存目录」一次性调用 `PUT /api/v1/admin/series/{id}/outline`，一次提交全部顺序调整。版本冲突时保留本地编排，展示最新目录供核对后重新提交。单条写入成功后同时刷新文章及系列缓存。

### 8.3 与现有文章页面联动

- **文章列表页（`/posts`）**：新增「所属系列」筛选器（含「未归属系列」选项），列表行内展示所属系列标签。
- **文章编辑页（`/posts/$postId/edit`）**：右侧设置面板新增「所属系列」卡片——未归属时可下拉选系列一键加入末尾；已归属时展示系列名与当前位置，提供「移出系列」与「前往系列目录」链接。

---

## 9. 批量导入工具联动规划（`upload-articles`）

现有脚本按源文件相对路径识别文章，成功结果在 `sync-report.json` 的 `completed[].file/postId` 中，工作流终点为文章已保存。系列同步作为可选后续步骤，不自动公开文章或系列。 [导入工具说明](../upload-articles/README.md)

1. 清单以唯一系列 slug 标识目标，指定主分类、篇章和笔记相对路径；显式数组顺序优先。目录扫描模式按文件名前缀**数值**排序，同号或无编号以规范化相对路径作稳定次序。以同步报告解析文章 ID；空文件、缺图、失败和未解析路径在预检中按系列列出。
2. 只有拿到一个系列的完整目标才能提交全量 PUT。`--file`、`--limit` 的部分报告不能作为整本目录；允许补跑文章后重新汇齐清单。`--dry-run` 显示建系列、加入、移除、移动、冲突和缺项，不发写请求。一个系列失败不阻塞其他完整系列。
3. 分页读取系列，按 slug **精确**匹配，缺失才 POST；并发创建返回 `slug_in_use` 时重新查询。保存服务端返回的系列 ID、篇章 ID、上次同步的规范化目录及编辑版本，用于区分同名篇章和后台后续改动。初次遇到已有非空目录且与目标不同时先报告差异，由操作者明确纳入同步。
4. 比较服务端现状、上次成功结果和清单。现状等于目标时直接成功；遇到未合并的人工改动或跨系列占用时报告冲突，保留现状。其余变更携当前 `expected_edit_version` 一次 PUT，回读核对再更新同步记录。响应丢失先回读，重复运行不得新增系列、重复挂载或产生多余的公开版本。

---

## 10. 分阶段实施与验收

| 切片 | 实施范围 | 可验证结果 |
| --- | --- | --- |
| 1. 迁移与权限 | 在媒体去重迁移后创建系列表、索引、引用视图及角色授权，同步数据库契约与 `schema_invariants.sql` | 空库和已有文章/媒体/线上版本的库均可升级；重复初始化权限不重复授权；跨系列篇章引用失败，删除篇章并把文章移到根后仍保留成员 |
| 2. 后台 API 与发布 | 实现事务、审计、冲突映射、文章筛选、媒体引用、公开投影与同步发布；更新 REST、响应、快照契约与 OpenAPI | 两个编辑器同时保存、全量保存撞上单条加入均只允许一个旧版本请求成功；两个系列抢同一篇仅一个成功；失败事务不改目录、版本和 job 数。覆盖 50/200 边界、多字节请求、无变化保存、401/403、Author 的过滤与引用可见性 |
| 3. 公开站点 | 更新 Rust 和 Astro 快照读者、示例数据、SSR 路由、导航、分类页与 sitemap | 旧 v1 无系列仍可渲染；从含草稿和空篇章的后台目录导出夹具，断言两者已过滤；新快照覆盖根条目、跨章、仅系列引用的分类与共用封面。核对编号与跨章上下篇、单篇边界、分类真实路径、空总览和移动端；损坏引用使离线构建失败或 SSR 返回 503 |
| 4. 管理后台 | 按 OpenAPI 重新生成客户端，接入路由、权限、工作台、文章编辑器、媒体引用与任务详情 | 从建系列、快捷新建、保存目录、公开直至站点上线；模拟创建成功但挂载失败后恢复。换封面成功后旧图无引用即可删。下线失败时原内容继续在线，成功后系列详情为 404，成员文章仍可访问 |
| 5. 导入（可选） | 实现清单预检、ID 映射、冲突与恢复报告 | 同一清单运行两次，第二次零写入；缺图、局部重跑、重名篇章、响应丢失和后台并发编辑均不丢失既有目录 |

**上线顺序。** 在升级已有异步站点前核对最新目标和线上版本。同步发布迁移会在二者不一致时中止，保留旧数据供处理。待目标与线上一致后，停用旧版写入和构建 Worker，执行迁移并部署 Rust API 与 Astro SSR，最后启用后台和可选的批量导入工具。

**实施验证入口。** 后端从 `server/` 对测试库运行 `make migrate`、`make check-schema`，执行 `cargo test --workspace --locked`，并显式运行标记为 `--ignored` 的数据库集成用例（含新增系列用例）；前端从 `frontend/` 运行 `pnpm generate:api`、`pnpm check:api`、`pnpm check`、`pnpm test` 和 `pnpm build`。导入切片另从 `upload-articles/` 运行 `pnpm typecheck` 与 `node --test *.test.ts`。以上为开发完成后的验收条件。
