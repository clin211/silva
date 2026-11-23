# Developer Hub 后端技术文档

- 版本：1.2
- 技术栈：Rust + Axum + Tokio + SQLx + PostgreSQL
- 架构：模块化单体 API + 独立 Worker
- 接口风格：REST + OpenAPI
- 文件存储：S3 兼容对象存储（如 Cloudflare R2）

## 1. 后端架构

```text
Public Website (Astro) ─┐
                        ├── REST API (Axum) ─── PostgreSQL
Admin SPA (React) ──────┘          │
                                  ├── S3/R2
                                  ├── Outbox Events
                                  └── Worker (Tokio)
                                        ├── AI ingestion
                                        ├── trend aggregation
                                        └── site build/deploy
```

API Server 负责请求、授权、业务用例和数据库事务。Worker 负责长任务、定时任务、外部 API 采集、趋势计算和站点构建。初期保持同一代码仓库和领域模型，但作为独立进程部署。

## 2. Rust Workspace

```text
backend/
├── Cargo.toml
├── apps/
│   ├── api/
│   │   └── src/main.rs
│   ├── worker/
│   │   └── src/main.rs
│   └── cli/
│       └── src/main.rs
├── crates/
│   ├── domain/
│   │   ├── src/
│   │   │   ├── identity/
│   │   │   ├── content/
│   │   │   ├── taxonomy/
│   │   │   ├── media/
│   │   │   ├── profile/
│   │   │   └── ai_trending/
│   ├── application/
│   │   └── src/
│   │       ├── commands/
│   │       ├── queries/
│   │       ├── ports/
│   │       └── jobs/
│   ├── infrastructure/
│   │   └── src/
│   │       ├── persistence/
│   │       ├── object_storage/
│   │       ├── github/
│   │       ├── publishing/
│   │       └── scheduler/
│   └── shared/
│       └── src/
│           ├── config.rs
│           ├── error.rs
│           ├── ids.rs
│           └── pagination.rs
├── migrations/
└── tests/
```

### 2.1 依赖方向

- `domain`：纯领域类型、规则和状态，不依赖 Axum、SQLx。
- `application`：用例编排，定义 repository、storage、provider 等 ports。
- `infrastructure`：SQLx、S3、GitHub API、发布服务等适配器。
- `apps/api`：HTTP 路由、认证中间件、DTO、OpenAPI。
- `apps/worker`：消费 Outbox、定时调度、采集和构建任务。
- `apps/cli`：迁移辅助、管理员初始化、运维命令。

不要让 domain 直接依赖 infrastructure。不要把所有业务逻辑写进 Axum handler。

## 3. 数据库设计约定

- PostgreSQL 为主数据库。
- 本系统分配的 ID 必须使用 UUID v7（UUIDv7，RFC 9562）；时间字段统一 `timestamptz`。
- 所有业务表包含必要的 `created_at`、`updated_at`。
- slug 建立唯一索引。
- JSONB 仅用于确实需要灵活扩展的外部原始数据或配置，不替代核心关系字段。
- 外部来源 ID 建唯一约束，避免重复采集。
- Markdown 原文存 PostgreSQL；媒体二进制放 S3/R2。
- 通过 SQLx migrations 管理 schema。

### 3.1 表与字段注释

- **每张业务表、每个业务字段都必须有非空数据库注释。** 关联表、会话表、任务表、审计表及其中的主键、外键和时间戳字段均不例外。PostgreSQL 系统表与 SQLx 等工具维护的表不在检查范围内。
- §4 的设计 DDL 和实际 SQLx migration 均使用 `COMMENT ON TABLE`、`COMMENT ON COLUMN`。新增表或字段、变更字段业务含义时，在同一次迁移中维护注释；Markdown 说明和 SQL 行内注释不能替代数据库注释。为已经部署的表补注释时，新增迁移，不修改已执行的迁移。
- 表注释说明一行记录代表什么；字段注释说明业务含义，按需注明状态取值、单位、时间口径和 `NULL` 的含义。注释不得包含密钥、凭据或真实敏感数据。
- CI 应在临时 PostgreSQL 执行全部迁移后，以 `obj_description`、`col_description` 检查业务表和未删除的用户字段。任何注释缺失或仅含空白时，输出具体表名或字段名并使检查失败。

### 3.2 ID 生成与约束

- 业务实体主键、引用这些实体的 ID、Outbox 事件 ID、审计日志 ID 和服务端 `request_id` 均使用 UUIDv7。服务端通过 `shared::ids` 统一生成，Rust 使用 `uuid` crate 的 `v7` 特性和 `Uuid::now_v7()`。API、Worker、CLI、种子和测试数据遵守同一规范；客户端不分配新实体 ID。需在事务中相互引用的新记录可预先生成 ID。
- PostgreSQL 仍用原生 `uuid` 存储，通过 `is_uuid_v7()` 校验所有 UUID 实体主键和无法用外键保证的 UUID 引用；普通外键及复合外键通过引用已校验的主键保证版本。不把普通 `uuid` 类型视为版本校验，也不依赖数据库版本特有的 UUIDv7 生成函数。
- 数据源原生标识（如 GitHub `external_id`）保持原格式。Slug、角色键、`generation`、`revision_no`、会话及租约令牌是路由键、业务序号或令牌，不是实体 ID；关联表复合键与 `site_state` 的单例键保持原结构。
- UUIDv7 自带时间信息，但不作为业务时间或授权依据；展示与排序以显式时间和版本字段为准。CI 应验证 v7 可写入、v4 和错误变体被迁移后的数据库拒绝，并覆盖 API 拒绝非 v7 ID 的行为。

## 4. 最终数据库 Schema

以下为逻辑基线。字段类型可按实现细节调整，所有外键删除策略需在 migration 中明确。UUIDv7 校验函数须在创建依赖它的表之前安装。

```sql
CREATE FUNCTION is_uuid_v7(input_id uuid) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
AS 'SELECT substr(input_id::text, 15, 1) = ''7''
       AND substr(input_id::text, 20, 1) IN (''8'', ''9'', ''a'', ''b'')';
```

### 4.1 Identity / RBAC

```sql
CREATE TABLE users (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  email text NOT NULL UNIQUE,
  password_hash text NOT NULL,
  display_name text NOT NULL,
  status text NOT NULL DEFAULT 'active',
  last_login_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE users IS '管理后台账户，每条记录对应一个可分配角色的用户。';
COMMENT ON COLUMN users.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN users.email IS '登录使用的唯一邮箱地址';
COMMENT ON COLUMN users.password_hash IS 'Argon2id 密码哈希，不保存明文';
COMMENT ON COLUMN users.display_name IS '后台显示的用户名称';
COMMENT ON COLUMN users.status IS '账户状态，用于判断账户是否可登录，默认 active';
COMMENT ON COLUMN users.last_login_at IS '最近一次成功登录时间，未登录过时为 NULL';
COMMENT ON COLUMN users.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN users.updated_at IS '该记录在本系统最近一次更新的时间。';

CREATE TABLE roles (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  key text NOT NULL UNIQUE,
  name text NOT NULL
);

COMMENT ON TABLE roles IS '后台角色定义，一期通过内置角色分配权限。';
COMMENT ON COLUMN roles.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN roles.key IS '用于权限配置的唯一角色键';
COMMENT ON COLUMN roles.name IS '后台显示的角色名称';

CREATE TABLE permissions (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  key text NOT NULL UNIQUE,
  description text
);

COMMENT ON TABLE permissions IS '后台权限定义，每条记录对应一项可授予的操作权限。';
COMMENT ON COLUMN permissions.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN permissions.key IS '服务端授权检查使用的唯一权限键';
COMMENT ON COLUMN permissions.description IS '该权限允许的操作说明，未填写时为 NULL';

CREATE TABLE user_roles (
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  PRIMARY KEY (user_id, role_id)
);

COMMENT ON TABLE user_roles IS '用户与角色的分配关系，每条记录表示对一个用户授予一个角色。';
COMMENT ON COLUMN user_roles.user_id IS '获得角色的用户 ID';
COMMENT ON COLUMN user_roles.role_id IS '授予该用户的角色 ID';

CREATE TABLE role_permissions (
  role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  permission_id uuid NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
  PRIMARY KEY (role_id, permission_id)
);

COMMENT ON TABLE role_permissions IS '角色与权限的分配关系，每条记录表示一个角色拥有一项权限。';
COMMENT ON COLUMN role_permissions.role_id IS '获得权限的角色 ID';
COMMENT ON COLUMN role_permissions.permission_id IS '授予该角色的权限 ID';

CREATE TABLE sessions (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash text NOT NULL UNIQUE,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz
);

COMMENT ON TABLE sessions IS '后台登录会话，每条记录对应一个服务端会话。';
COMMENT ON COLUMN sessions.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN sessions.user_id IS '会话所属的后台用户 ID';
COMMENT ON COLUMN sessions.token_hash IS '用于查找会话的令牌哈希，不保存原始令牌';
COMMENT ON COLUMN sessions.expires_at IS '会话失效时间';
COMMENT ON COLUMN sessions.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN sessions.last_seen_at IS '最近一次记录的会话活动时间，未记录时为 NULL';
```

### 4.2 Content / Markdown CMS

```sql
CREATE TABLE posts (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  slug text NOT NULL UNIQUE,
  post_type text NOT NULL DEFAULT 'article',
  current_revision_id uuid NOT NULL,
  desired_published_revision_id uuid,
  publication_version bigint NOT NULL DEFAULT 0,
  slug_locked boolean NOT NULL DEFAULT false,
  author_id uuid REFERENCES users(id),
  desired_published_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((desired_published_revision_id IS NULL) = (desired_published_at IS NULL))
);

COMMENT ON TABLE posts IS '文章主记录，跟踪当前编辑目标和期望公开目标，不直接代表线上内容。';
COMMENT ON COLUMN posts.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN posts.slug IS '文章的唯一公开路由标识，首发请求被接受后锁定';
COMMENT ON COLUMN posts.post_type IS '内容类型，一期默认 article';
COMMENT ON COLUMN posts.current_revision_id IS '当前编辑修订 ID，必须属于本篇文章';
COMMENT ON COLUMN posts.desired_published_revision_id IS '期望公开的修订 ID，NULL 表示没有公开目标，不代表已撤下';
COMMENT ON COLUMN posts.publication_version IS '发布和取消发布共用的乐观锁版本号，每次成功变更时递增';
COMMENT ON COLUMN posts.slug_locked IS '文章路由是否已锁定，取消发布不解锁';
COMMENT ON COLUMN posts.author_id IS '用于文章归属授权的作者用户 ID，NULL 表示没有关联用户';
COMMENT ON COLUMN posts.desired_published_at IS '期望公开版本被接受的时间，未设公开目标时为 NULL，不代表线上切换时间';
COMMENT ON COLUMN posts.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN posts.updated_at IS '该记录在本系统最近一次更新的时间。';

CREATE TABLE post_revisions (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  revision_no integer NOT NULL,
  title text NOT NULL,
  excerpt text,
  markdown_body text NOT NULL,
  rendered_html text,
  cover_media_id uuid,
  is_featured boolean NOT NULL DEFAULT false,
  seo_title text,
  seo_description text,
  created_by uuid REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (post_id, revision_no),
  UNIQUE (post_id, id)
);

COMMENT ON TABLE post_revisions IS '文章的不可变修订，每次保存产生一条记录。';
COMMENT ON COLUMN post_revisions.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN post_revisions.post_id IS '修订所属的文章 ID';
COMMENT ON COLUMN post_revisions.revision_no IS '同一篇文章内唯一的修订序号';
COMMENT ON COLUMN post_revisions.title IS '该修订的文章标题';
COMMENT ON COLUMN post_revisions.excerpt IS '该修订的摘要，未填写时为 NULL';
COMMENT ON COLUMN post_revisions.markdown_body IS '用于编辑的 Markdown 正文原文';
COMMENT ON COLUMN post_revisions.rendered_html IS '经过消毒的 HTML 缓存，未缓存时为 NULL';
COMMENT ON COLUMN post_revisions.cover_media_id IS '该修订引用的已校验封面媒体 ID，未设置时为 NULL';
COMMENT ON COLUMN post_revisions.is_featured IS '该修订上线后是否作为首页精选文章';
COMMENT ON COLUMN post_revisions.seo_title IS '自定义 SEO 标题，NULL 表示未覆盖默认值';
COMMENT ON COLUMN post_revisions.seo_description IS '自定义 SEO 描述，NULL 表示未覆盖默认值';
COMMENT ON COLUMN post_revisions.created_by IS '保存该修订的后台用户 ID，无关联用户时为 NULL';
COMMENT ON COLUMN post_revisions.created_at IS '该记录首次写入本系统的时间。';

ALTER TABLE posts
  ADD CONSTRAINT posts_current_revision_fk
  FOREIGN KEY (id, current_revision_id)
  REFERENCES post_revisions(post_id, id)
  DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE posts
  ADD CONSTRAINT posts_desired_revision_fk
  FOREIGN KEY (id, desired_published_revision_id)
  REFERENCES post_revisions(post_id, id)
  DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE categories (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  name text NOT NULL,
  slug text NOT NULL UNIQUE,
  description text,
  created_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE categories IS '博客分类字典，每条记录对应一个可被文章修订引用的分类。';
COMMENT ON COLUMN categories.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN categories.name IS '向读者显示的分类名称';
COMMENT ON COLUMN categories.slug IS '用于公开页面路由的唯一标识。';
COMMENT ON COLUMN categories.description IS '可选的分类说明，未填写时为 NULL';
COMMENT ON COLUMN categories.created_at IS '该记录首次写入本系统的时间。';

CREATE TABLE tags (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  name text NOT NULL,
  slug text NOT NULL UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE tags IS '博客标签字典，每条记录对应一个可被文章修订引用的标签。';
COMMENT ON COLUMN tags.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN tags.name IS '向读者显示的标签名称';
COMMENT ON COLUMN tags.slug IS '用于公开页面路由的唯一标识。';
COMMENT ON COLUMN tags.created_at IS '该记录首次写入本系统的时间。';

CREATE TABLE post_revision_categories (
  revision_id uuid NOT NULL REFERENCES post_revisions(id) ON DELETE CASCADE,
  category_id uuid NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
  PRIMARY KEY (revision_id, category_id)
);

COMMENT ON TABLE post_revision_categories IS '文章修订与分类的关联，冻结该修订选择的分类。';
COMMENT ON COLUMN post_revision_categories.revision_id IS '引用分类的文章修订 ID';
COMMENT ON COLUMN post_revision_categories.category_id IS '该修订选用的分类 ID';

CREATE TABLE post_revision_tags (
  revision_id uuid NOT NULL REFERENCES post_revisions(id) ON DELETE CASCADE,
  tag_id uuid NOT NULL REFERENCES tags(id) ON DELETE RESTRICT,
  PRIMARY KEY (revision_id, tag_id)
);

COMMENT ON TABLE post_revision_tags IS '文章修订与标签的关联，冻结该修订选择的标签。';
COMMENT ON COLUMN post_revision_tags.revision_id IS '引用标签的文章修订 ID';
COMMENT ON COLUMN post_revision_tags.tag_id IS '该修订选用的标签 ID';

CREATE TABLE post_reviews (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  post_id uuid NOT NULL,
  revision_id uuid NOT NULL,
  reviewer_id uuid REFERENCES users(id),
  status text NOT NULL CHECK (status IN ('pending', 'approved', 'rejected')),
  comment text,
  reviewed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (post_id, revision_id),
  FOREIGN KEY (post_id, revision_id) REFERENCES post_revisions(post_id, id) ON DELETE CASCADE
);

COMMENT ON TABLE post_reviews IS '文章修订的审核记录，每个修订最多提交一次审核。';
COMMENT ON COLUMN post_reviews.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN post_reviews.post_id IS '被审核修订所属的文章 ID';
COMMENT ON COLUMN post_reviews.revision_id IS '本次审核针对的不可变修订 ID';
COMMENT ON COLUMN post_reviews.reviewer_id IS '作出审核决定的用户 ID，待审核时为 NULL';
COMMENT ON COLUMN post_reviews.status IS '审核状态：pending 待审核、approved 已批准、rejected 已拒绝';
COMMENT ON COLUMN post_reviews.comment IS '审核意见，未填写时为 NULL';
COMMENT ON COLUMN post_reviews.reviewed_at IS '作出审核决定的时间，待审核时为 NULL';
COMMENT ON COLUMN post_reviews.created_at IS '该修订提交审核的时间';
```

约束说明：

- 创建文章时预分配首条 revision ID，在一个事务内写入 post、revision 及关联；此后 revision、分类标签选择、封面和媒体引用只追加。`rendered_html` 若保存，应在创建 revision 时生成并消毒；公开导出只用安全 HTML，不透传 Markdown 原文。
- `current_revision_id` 只表示当前编辑版本。`desired_published_revision_id` 只表示下一次站点构建应采用的目标；当前线上的 revision 由 `active_generation` 的 manifest 决定。三者独立，不用单一文章状态表达。
- 一期每个 revision 最多提交一次；审核由 `pending` 转为 `approved` 或 `rejected`，拒绝后需要创建新 revision 再送审。新 revision 不继承旧批准，已批准的历史 revision 可重新发布。
- 首次接受发布后锁定文章 slug，取消发布不解锁；一期分类、标签及其他详情实体的 slug 创建后也不改名。分类、标签名称可独立修改并进入站点发布流程；有历史 revision 引用时不能删除（`409 taxonomy_in_use`）。

### 4.3 Media

```sql
CREATE TABLE media_assets (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  upload_key text NOT NULL UNIQUE,
  storage_key text UNIQUE,
  status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'rejected')),
  original_filename text NOT NULL,
  mime_type text,
  size_bytes bigint CHECK (size_bytes > 0),
  width integer,
  height integer,
  alt_text text,
  upload_expires_at timestamptz NOT NULL,
  uploaded_by uuid REFERENCES users(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (status <> 'ready' OR (storage_key IS NOT NULL AND mime_type IS NOT NULL AND size_bytes IS NOT NULL))
);

COMMENT ON TABLE media_assets IS '媒体上传及校验记录，仅校验通过的记录可供业务引用。';
COMMENT ON COLUMN media_assets.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN media_assets.upload_key IS '预签名直传使用的私有临时对象键';
COMMENT ON COLUMN media_assets.storage_key IS '校验后写入的不可覆盖私有对象键，未写入时为 NULL';
COMMENT ON COLUMN media_assets.status IS '媒体状态：pending 待校验、ready 已通过、rejected 已拒绝';
COMMENT ON COLUMN media_assets.original_filename IS '上传方提供的原文件名，仅用于显示，不作为存储路径';
COMMENT ON COLUMN media_assets.mime_type IS '服务端校验得到的 MIME 类型，未确认时为 NULL';
COMMENT ON COLUMN media_assets.size_bytes IS '服务端校验得到的实际大小，单位为字节，未确认时为 NULL';
COMMENT ON COLUMN media_assets.width IS '图片宽度，单位为像素，未取得尺寸时为 NULL';
COMMENT ON COLUMN media_assets.height IS '图片高度，单位为像素，未取得尺寸时为 NULL';
COMMENT ON COLUMN media_assets.alt_text IS '图片无障碍替代文本，未填写时为 NULL';
COMMENT ON COLUMN media_assets.upload_expires_at IS '临时直传和上传确认使用的过期时间';
COMMENT ON COLUMN media_assets.uploaded_by IS '发起上传的后台用户 ID，无关联用户时为 NULL';
COMMENT ON COLUMN media_assets.created_at IS '该记录首次写入本系统的时间。';

ALTER TABLE post_revisions
  ADD CONSTRAINT post_revisions_cover_media_fk
  FOREIGN KEY (cover_media_id) REFERENCES media_assets(id) ON DELETE RESTRICT;

CREATE TABLE post_revision_media (
  revision_id uuid NOT NULL REFERENCES post_revisions(id) ON DELETE CASCADE,
  media_id uuid NOT NULL REFERENCES media_assets(id) ON DELETE RESTRICT,
  PRIMARY KEY (revision_id, media_id)
);

COMMENT ON TABLE post_revision_media IS '文章修订正文与媒体的关联，用于校验引用及阻止误删。';
COMMENT ON COLUMN post_revision_media.revision_id IS '使用该媒体的文章修订 ID';
COMMENT ON COLUMN post_revision_media.media_id IS '该修订正文引用的已校验媒体 ID';
```

一期只接收图片 JPEG、PNG、WebP，单文件上限 10 MiB，并限制解码像素数。预签名只允许上传到短时有效的私有临时 key。确认时检查归属、有效期、实际字节大小与图像解码结果；校验的字节写入独立且不可覆盖的私有正式 key 后，才能标记 `ready`，重复确认返回同一结果。仅 `ready` 且调用者有权引用的媒体可用于文章封面、正文或个人头像。正文图片只允许受控媒体 ID 引用，保存时提取并持久化引用；管理预览和公开渲染分别解析为对应的授权读取路径。后台媒体读取要求授权，公开读取只允许当前 manifest 包含的媒体 ID。清理弃置的临时对象；删除正式对象前须确认当前及历史文章修订、个人资料、当前线上或待切换产物均无引用。

### 4.4 个人资料

```sql
CREATE TABLE profiles (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  display_name text NOT NULL,
  headline text,
  bio_md text,
  avatar_media_id uuid REFERENCES media_assets(id),
  location_label text,
  updated_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE profiles IS '前台使用的个人资料主记录，一期由后台按单例管理。';
COMMENT ON COLUMN profiles.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN profiles.display_name IS '前台显示的个人名称';
COMMENT ON COLUMN profiles.headline IS '公开头衔或一句话介绍，未填写时为 NULL';
COMMENT ON COLUMN profiles.bio_md IS '个人介绍的 Markdown 正文，未填写时为 NULL';
COMMENT ON COLUMN profiles.avatar_media_id IS '已校验的头像媒体 ID，未设置时为 NULL';
COMMENT ON COLUMN profiles.location_label IS '公开展示的地点文案，未填写时为 NULL';
COMMENT ON COLUMN profiles.updated_at IS '该记录在本系统最近一次更新的时间。';

CREATE TABLE experiences (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  profile_id uuid NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  organization text NOT NULL,
  role text NOT NULL,
  description_md text,
  started_on date,
  ended_on date,
  position integer NOT NULL DEFAULT 0
);

COMMENT ON TABLE experiences IS '个人资料中的经历条目，每条记录对应一段经历。';
COMMENT ON COLUMN experiences.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN experiences.profile_id IS '该经历所属的个人资料 ID';
COMMENT ON COLUMN experiences.organization IS '经历对应的组织名称';
COMMENT ON COLUMN experiences.role IS '该段经历中的职位或角色';
COMMENT ON COLUMN experiences.description_md IS '经历的 Markdown 说明，未填写时为 NULL';
COMMENT ON COLUMN experiences.started_on IS '经历开始日期，未知时为 NULL';
COMMENT ON COLUMN experiences.ended_on IS '经历结束日期，尚未结束或未知时为 NULL';
COMMENT ON COLUMN experiences.position IS '该经历在个人资料中的排序值';

CREATE TABLE skills (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  profile_id uuid NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  name text NOT NULL,
  category text,
  proficiency_label text,
  position integer NOT NULL DEFAULT 0
);

COMMENT ON TABLE skills IS '个人资料中的技能条目，每条记录对应一项技能。';
COMMENT ON COLUMN skills.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN skills.profile_id IS '技能所属的个人资料 ID';
COMMENT ON COLUMN skills.name IS '公开展示的技能名称';
COMMENT ON COLUMN skills.category IS '技能分组名称，未分组时为 NULL';
COMMENT ON COLUMN skills.proficiency_label IS '公开的熟练程度文案，未填写时为 NULL';
COMMENT ON COLUMN skills.position IS '技能在个人资料中的排序值';

CREATE TABLE social_links (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  profile_id uuid NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  platform text NOT NULL,
  label text,
  url text NOT NULL,
  position integer NOT NULL DEFAULT 0
);

COMMENT ON TABLE social_links IS '个人资料公开展示的社交或外部主页链接。';
COMMENT ON COLUMN social_links.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN social_links.profile_id IS '链接所属的个人资料 ID';
COMMENT ON COLUMN social_links.platform IS '链接对应的平台标识';
COMMENT ON COLUMN social_links.label IS '自定义链接文案，未填写时为 NULL';
COMMENT ON COLUMN social_links.url IS '公开主页或社交账号的访问地址';
COMMENT ON COLUMN social_links.position IS '链接在个人资料中的排序值';
```

### 4.5 AI Trending 自动采集

```sql
CREATE TABLE ai_sources (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  key text NOT NULL UNIQUE,
  name text NOT NULL,
  base_url text,
  enabled boolean NOT NULL DEFAULT true,
  config jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE ai_sources IS 'AI Trending 的上游数据源配置，每条记录对应一个数据源。';
COMMENT ON COLUMN ai_sources.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN ai_sources.key IS '系统识别数据源的唯一键';
COMMENT ON COLUMN ai_sources.name IS '后台显示的数据源名称';
COMMENT ON COLUMN ai_sources.base_url IS '上游 API 基础地址，使用默认地址时可为 NULL';
COMMENT ON COLUMN ai_sources.enabled IS '是否启用该数据源的采集';
COMMENT ON COLUMN ai_sources.config IS 'topic 白名单及采集上限等配置，不保存凭据';
COMMENT ON COLUMN ai_sources.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN ai_sources.updated_at IS '该记录在本系统最近一次更新的时间。';

CREATE TABLE ai_repositories (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  source_id uuid NOT NULL REFERENCES ai_sources(id),
  external_id text NOT NULL,
  owner text NOT NULL,
  name text NOT NULL,
  full_name text NOT NULL,
  slug text NOT NULL UNIQUE,
  repository_url text NOT NULL,
  description text,
  primary_language text,
  license_spdx text,
  topics text[] NOT NULL DEFAULT '{}',
  is_hidden boolean NOT NULL DEFAULT false,
  is_featured boolean NOT NULL DEFAULT false,
  manual_category text,
  upstream_created_at timestamptz NOT NULL,
  upstream_updated_at timestamptz NOT NULL,
  first_seen_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (source_id, external_id)
);

COMMENT ON TABLE ai_repositories IS 'AI Trending 收录的外部仓库，保存身份、当前元数据和人工展示设置。';
COMMENT ON COLUMN ai_repositories.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN ai_repositories.source_id IS '仓库所属的数据源 ID';
COMMENT ON COLUMN ai_repositories.external_id IS '数据源内稳定的仓库标识，仓库改名后仍用于去重';
COMMENT ON COLUMN ai_repositories.owner IS '上游仓库所有者名称';
COMMENT ON COLUMN ai_repositories.name IS '上游仓库名称';
COMMENT ON COLUMN ai_repositories.full_name IS '上游完整仓库名称，GitHub 格式为 owner/name';
COMMENT ON COLUMN ai_repositories.slug IS '用于公开页面路由的唯一标识。';
COMMENT ON COLUMN ai_repositories.repository_url IS '上游仓库的访问地址';
COMMENT ON COLUMN ai_repositories.description IS '上游仓库描述，未提供时为 NULL';
COMMENT ON COLUMN ai_repositories.primary_language IS '上游报告的主要语言，未知时为 NULL';
COMMENT ON COLUMN ai_repositories.license_spdx IS '上游许可证的 SPDX 标识，未知时为 NULL';
COMMENT ON COLUMN ai_repositories.topics IS '上游标注的主题列表，空数组表示未记录主题';
COMMENT ON COLUMN ai_repositories.is_hidden IS '是否从公开榜单和详情中排除';
COMMENT ON COLUMN ai_repositories.is_featured IS '是否进入独立的精选区域，不改变自然排名';
COMMENT ON COLUMN ai_repositories.manual_category IS '人工修正的公开分类，NULL 表示无人工覆盖，采集不得修改';
COMMENT ON COLUMN ai_repositories.upstream_created_at IS '仓库在外部平台的创建时间，不是本地入库时间';
COMMENT ON COLUMN ai_repositories.upstream_updated_at IS '上游报告的仓库更新时间，用于最近更新排序';
COMMENT ON COLUMN ai_repositories.first_seen_at IS '本地采集首次收录仓库的时间';
COMMENT ON COLUMN ai_repositories.last_seen_at IS '本地最近一次在采集中观察到仓库的时间，尚未记录时为 NULL';
COMMENT ON COLUMN ai_repositories.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN ai_repositories.updated_at IS '该记录在本系统最近一次更新的时间。';

CREATE TABLE ai_ingestion_runs (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  source_id uuid NOT NULL REFERENCES ai_sources(id),
  slot_date date NOT NULL, -- UTC 逻辑采集日
  status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  attempts integer NOT NULL DEFAULT 0,
  lease_token uuid,
  lease_expires_at timestamptz,
  started_at timestamptz,
  finished_at timestamptz,
  items_seen integer NOT NULL DEFAULT 0,
  items_created integer NOT NULL DEFAULT 0,
  snapshots_created integer NOT NULL DEFAULT 0,
  error_message text,
  metadata jsonb NOT NULL DEFAULT '{}',
  UNIQUE (source_id, slot_date)
);

COMMENT ON TABLE ai_ingestion_runs IS '某数据源每个 UTC 采集日对应的一条批次记录，重试复用该记录。';
COMMENT ON COLUMN ai_ingestion_runs.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN ai_ingestion_runs.source_id IS '批次所属的数据源 ID';
COMMENT ON COLUMN ai_ingestion_runs.slot_date IS '使用 UTC 划分的逻辑采集日，用于幂等和增长窗口计算';
COMMENT ON COLUMN ai_ingestion_runs.status IS '批次状态：queued 排队、running 运行中、succeeded 成功、failed 失败';
COMMENT ON COLUMN ai_ingestion_runs.attempts IS '该批次的执行尝试次数';
COMMENT ON COLUMN ai_ingestion_runs.lease_token IS '当前执行者的租约令牌，无租约时为 NULL';
COMMENT ON COLUMN ai_ingestion_runs.lease_expires_at IS '执行租约的截止时间，无租约时为 NULL';
COMMENT ON COLUMN ai_ingestion_runs.started_at IS '开始执行的时间，未领取时为 NULL';
COMMENT ON COLUMN ai_ingestion_runs.finished_at IS '执行结束时间，未结束时为 NULL';
COMMENT ON COLUMN ai_ingestion_runs.items_seen IS '本批次观察到的项目数';
COMMENT ON COLUMN ai_ingestion_runs.items_created IS '本批次新增的本地项目数';
COMMENT ON COLUMN ai_ingestion_runs.snapshots_created IS '本批次落库的快照数';
COMMENT ON COLUMN ai_ingestion_runs.error_message IS '失败原因摘要，无错误时为 NULL，不记录凭据';
COMMENT ON COLUMN ai_ingestion_runs.metadata IS '限流等补充运行信息，不存储凭据';

CREATE TABLE ai_repository_snapshots (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  run_id uuid NOT NULL REFERENCES ai_ingestion_runs(id) ON DELETE RESTRICT,
  repository_id uuid NOT NULL REFERENCES ai_repositories(id) ON DELETE CASCADE,
  stars_count bigint NOT NULL CHECK (stars_count >= 0),
  forks_count bigint NOT NULL CHECK (forks_count >= 0),
  open_issues_count bigint NOT NULL CHECK (open_issues_count >= 0),
  pushed_at timestamptz,
  latest_release_tag text,
  snapshot_at timestamptz NOT NULL,
  raw_payload jsonb,
  UNIQUE (run_id, repository_id)
);

COMMENT ON TABLE ai_repository_snapshots IS '成功批次中某个项目的指标快照，每项目每批次最多一条。';
COMMENT ON COLUMN ai_repository_snapshots.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN ai_repository_snapshots.run_id IS '生成快照的成功批次 ID';
COMMENT ON COLUMN ai_repository_snapshots.repository_id IS '指标所属的仓库 ID';
COMMENT ON COLUMN ai_repository_snapshots.stars_count IS '观测时上游的 Stars 总数';
COMMENT ON COLUMN ai_repository_snapshots.forks_count IS '观测时上游的 Forks 总数';
COMMENT ON COLUMN ai_repository_snapshots.open_issues_count IS '观测时上游 API 报告的 Open Issues 数量';
COMMENT ON COLUMN ai_repository_snapshots.pushed_at IS '上游报告的最近推送时间，未知时为 NULL';
COMMENT ON COLUMN ai_repository_snapshots.latest_release_tag IS '观测时最近可获取的发布标签，没有时为 NULL';
COMMENT ON COLUMN ai_repository_snapshots.snapshot_at IS '指标实际观测时间，不替代批次的 UTC 采集日';
COMMENT ON COLUMN ai_repository_snapshots.raw_payload IS '可选的原始上游响应，不向公开接口输出';

CREATE INDEX ai_snapshots_repo_time_idx
  ON ai_repository_snapshots(repository_id, snapshot_at DESC);
CREATE INDEX ai_repositories_featured_idx
  ON ai_repositories(is_featured) WHERE is_hidden = false;
```

设计说明：

- `ai_repositories` 保存上游项目元数据和独立的人工展示标记；采集 upsert 不更新人工字段。`created_at`、`updated_at` 是本地记录时间；上游时间单独存储。
- `ai_ingestion_runs` 记录每日批次；`ai_repository_snapshots` 关联批次且只追加成功样本。写入用例校验批次与仓库属于同一数据源。窗口和排序见 §7.1。
- `ai_sources.config` 保存 topic 白名单和采集上限等配置；凭据放 Secret/环境变量，不进入数据库配置或公开 API。一期只启用 GitHub 数据源。

### 4.6 发布、Outbox、审计

```sql
CREATE TABLE publish_jobs (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  generation bigint NOT NULL UNIQUE CHECK (generation > 0),
  status text NOT NULL CHECK (status IN ('queued', 'building', 'active', 'failed', 'superseded', 'retired')),
  trigger_type text NOT NULL,
  requested_by uuid REFERENCES users(id),
  manifest_key text,
  artifact_prefix text,
  attempts integer NOT NULL DEFAULT 0,
  lease_token uuid,
  lease_expires_at timestamptz,
  started_at timestamptz,
  finished_at timestamptz,
  error_message text,
  created_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE publish_jobs IS '站点发布任务，每条记录跟踪一个 generation 的构建与线上切换。';
COMMENT ON COLUMN publish_jobs.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN publish_jobs.generation IS '站点版本号，随新目标递增，一个版本仅有一个任务';
COMMENT ON COLUMN publish_jobs.status IS '任务状态：queued 排队、building 构建中、active 已上线、failed 失败、superseded 已被取代、retired 已退役';
COMMENT ON COLUMN publish_jobs.trigger_type IS '触发发布的业务事件类型';
COMMENT ON COLUMN publish_jobs.requested_by IS '触发操作的后台用户 ID，自动触发时可为 NULL';
COMMENT ON COLUMN publish_jobs.manifest_key IS '此版本公开数据投影的私有对象键，尚未登记时为 NULL';
COMMENT ON COLUMN publish_jobs.artifact_prefix IS '此版本静态产物的私有存储前缀，尚未登记时为 NULL';
COMMENT ON COLUMN publish_jobs.attempts IS '该 generation 的构建尝试次数';
COMMENT ON COLUMN publish_jobs.lease_token IS '构建 Worker 的租约令牌，无租约时为 NULL';
COMMENT ON COLUMN publish_jobs.lease_expires_at IS '构建租约截止时间，无租约时为 NULL';
COMMENT ON COLUMN publish_jobs.started_at IS '开始构建的时间，未领取时为 NULL';
COMMENT ON COLUMN publish_jobs.finished_at IS '任务结束时间，未结束时为 NULL';
COMMENT ON COLUMN publish_jobs.error_message IS '失败原因摘要，无错误时为 NULL，不记录凭据';
COMMENT ON COLUMN publish_jobs.created_at IS '该记录首次写入本系统的时间。';

CREATE TABLE site_state (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  desired_generation bigint REFERENCES publish_jobs(generation),
  active_generation bigint REFERENCES publish_jobs(generation),
  activated_at timestamptz,
  CHECK (active_generation IS NULL OR (desired_generation IS NOT NULL AND active_generation <= desired_generation))
);

COMMENT ON TABLE site_state IS '站点发布状态单例，记录最新提交目标与统一的线上读取指针。';
COMMENT ON COLUMN site_state.singleton IS '固定为 true 的主键，限制表最多一条记录';
COMMENT ON COLUMN site_state.desired_generation IS '最新提交的站点目标版本，尚未提交发布时为 NULL';
COMMENT ON COLUMN site_state.active_generation IS '静态站点与 Public API 共同读取的当前线上版本，首次上线前为 NULL';
COMMENT ON COLUMN site_state.activated_at IS '当前线上版本的实际切换时间，首次上线前为 NULL';

INSERT INTO site_state (singleton) VALUES (true);

CREATE TABLE outbox_events (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  aggregate_type text NOT NULL,
  aggregate_id uuid NOT NULL CHECK (is_uuid_v7(aggregate_id)),
  event_type text NOT NULL,
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz,
  attempts integer NOT NULL DEFAULT 0,
  last_error text
);

COMMENT ON TABLE outbox_events IS '随业务事务持久化并供 Worker 异步处理的领域事件。';
COMMENT ON COLUMN outbox_events.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN outbox_events.aggregate_type IS '产生事件的业务实体类型';
COMMENT ON COLUMN outbox_events.aggregate_id IS '产生事件的业务实体 UUIDv7；站点发布事件填写 publish_jobs.id，不填写 generation。';
COMMENT ON COLUMN outbox_events.event_type IS '消费者用于分派处理的事件类型';
COMMENT ON COLUMN outbox_events.payload IS '事件业务载荷，例如发布事件携带 generation，不存储凭据';
COMMENT ON COLUMN outbox_events.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN outbox_events.processed_at IS '消费者确认处理的时间，尚未确认时为 NULL';
COMMENT ON COLUMN outbox_events.attempts IS '事件处理的尝试次数';
COMMENT ON COLUMN outbox_events.last_error IS '最近一次事件处理失败的摘要，无错误时为 NULL';

CREATE INDEX outbox_unprocessed_idx
  ON outbox_events(created_at) WHERE processed_at IS NULL;

CREATE TABLE audit_logs (
  id uuid PRIMARY KEY CHECK (is_uuid_v7(id)),
  actor_user_id uuid REFERENCES users(id),
  action text NOT NULL,
  resource_type text NOT NULL,
  resource_id uuid CHECK (is_uuid_v7(resource_id)),
  ip_address inet,
  user_agent text,
  metadata jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE audit_logs IS '管理操作的审计记录，用于追踪操作主体、行为及目标。';
COMMENT ON COLUMN audit_logs.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN audit_logs.actor_user_id IS '执行操作的后台用户 ID，系统操作可为 NULL';
COMMENT ON COLUMN audit_logs.action IS '被记录的操作类型';
COMMENT ON COLUMN audit_logs.resource_type IS '操作涉及的资源类型';
COMMENT ON COLUMN audit_logs.resource_id IS '目标实体的 UUIDv7，不涉及单个实体时为 NULL。';
COMMENT ON COLUMN audit_logs.ip_address IS '记录到的客户端 IP，无法获取时为 NULL';
COMMENT ON COLUMN audit_logs.user_agent IS '客户端请求报告的 User-Agent，未提供时为 NULL';
COMMENT ON COLUMN audit_logs.metadata IS '审计附加信息，不记录密码、会话令牌或凭据';
COMMENT ON COLUMN audit_logs.created_at IS '该记录首次写入本系统的时间。';
```

## 5. API 设计约定

### 5.1 API 分组

- `/api/v1/public/*`：公开只读接口。
- `/api/v1/admin/*`：需要登录和 RBAC 的管理接口。
- `/api/v1/internal/*`：Worker/内部任务接口，仅内网或服务身份访问。

业务请求与响应使用 JSON（204 无响应体；媒体直传和媒体二进制读取例外）。统一分页、错误结构、排序和过滤参数，不直接暴露数据库模型。列表默认 `page=1`、`per_page=20`、最大 100；排序字段按端点列入 OpenAPI 白名单，最后使用稳定 ID 打破同分。OpenAPI 是运行时契约与 TypeScript Client 的来源；联调前将路径、权限、DTO、状态码及并发错误写入 OpenAPI。

业务实体 ID 在路径、查询参数和 JSON 中使用标准连字符 UUID 字符串。OpenAPI 对这些 ID 声明 `type: string`、`format: uuid`，并用 `pattern` 约束 UUIDv7 的版本位和变体位；服务端实际校验，格式或版本不符时返回 422 `validation_error`，字段错误码为 `invalid_format`。

### 5.2 统一响应约定

完整规范见 [API 响应结构规范](../server/docs/contracts/api-response.md)。成功响应统一以 `data` 包裹，分页信息放在 `meta`；204 没有正文。分页示例：

```json
{
  "data": [],
  "meta": {
    "pagination": {
      "page": 1,
      "per_page": 20,
      "total": 0,
      "total_pages": 0
    }
  }
}
```

错误结构：

```json
{
  "error": {
    "code": "validation_error",
    "message": "Request validation failed",
    "details": [
      { "field": "roles[1]", "code": "duplicate", "message": "role is listed more than once" }
    ],
    "request_id": "017f22e2-79b0-7cc3-98c4-dc0c0c07398f"
  }
}
```

常见 HTTP 状态：200、201、202、204、400、401、403、404、409、422、429、500、503。400 表示请求体不是合法 JSON；401 表示无有效会话，403 表示无权限或 CSRF 校验失败；409 表示资源当前状态或版本不允许该操作；422 表示输入字段无效，`details` 列出字段级错误。错误用稳定的 `error.code` 区分，详见 §6。

### 5.3 Public API

只读取当前 `active_generation` 的不可变公开投影（见 §7.2），不读草稿或期望发布数据。每个请求固定本次读取的 generation，并在响应返回 `X-Site-Generation`。客户端可传 `X-Expected-Generation`；若不一致返回 `409 site_generation_changed`，不提供访问旧 generation 的公开入口。尚无任何上线版本时返回 `503 site_not_ready`，不能假装数据为空。

| Method | Endpoint | 说明 |
| ------ | -------- | ---- |
| GET | `/api/v1/public/site` | 首页所需站点配置 |
| GET | `/api/v1/public/posts` | 线上文章列表，可按 `category` 或 `tag` slug 筛选 |
| GET | `/api/v1/public/posts/{slug}` | 线上文章详情，不可见时返回 404 |
| GET | `/api/v1/public/categories` | 分类列表 |
| GET | `/api/v1/public/tags` | 标签列表 |
| GET | `/api/v1/public/profile` | 个人资料 |
| GET | `/api/v1/public/ai-trending` | AI 趋势榜单 |
| GET | `/api/v1/public/ai-trending/{slug}` | AI 仓库详情 |
| GET | `/api/v1/public/ai-trending/{slug}/snapshots` | 当前可见仓库最近 90 天的成功快照 |
| GET | `/api/v1/public/media/{id}` | 读取当前公开投影引用的媒体内容，其他 ID 返回 404 |

### 5.4 Admin API

下表路径相对 `/api/v1/admin`。`CRUD` 仅为表内简写：集合 `GET/POST`、实体 `GET/PATCH/DELETE`，OpenAPI 中逐条定义。

| Method      | Path                                                                                                      | 说明                                           |
| ----------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| POST        | `/auth/login`                                                                                             | 公开入口；创建会话并返回 `csrf_token`          |
| POST        | `/auth/logout`                                                                                            | 撤销当前会话；204                              |
| GET         | `/auth/me`                                                                                                | 当前用户、权限和 `csrf_token`                  |
| GET, POST   | `/posts`                                                                                                  | 列表及创建；Author 只看到自己的文章            |
| GET         | `/posts/{id}`                                                                                             | 文章详情，含当前、期望、线上三个 revision ID   |
| POST        | `/posts/{id}/revisions`                                                                                   | 创建修订，见 §6.1                              |
| GET         | `/posts/{id}/revisions`、`/posts/{id}/revisions/{revision_id}`                                            | 修订列表及详情，供编辑、预览和历史查看         |
| POST        | `/posts/{id}/submit-review`                                                                               | Author 可提交自己的指定修订                    |
| POST        | `/posts/{id}/reviews`                                                                                     | Editor/Admin 批准或拒绝指定修订                |
| POST        | `/posts/{id}/publish`、`/posts/{id}/unpublish`                                                            | Editor/Admin 提交上线或下线目标，见 §6.2       |
| CRUD        | `/categories`、`/tags`                                                                                    | 分类标签维护；有引用时不允许删除               |
| POST        | `/media/presign`、`/media/{id}/complete`                                                                  | 申请和确认上传，见 §6.4                        |
| GET, DELETE | `/media/{id}`                                                                                             | 媒体元数据和授权删除；列表使用 `GET /media`    |
| GET         | `/media/{id}/content`                                                                                     | 授权读取已校验文件，管理域不缓存               |
| GET, PUT    | `/profile`                                                                                                | 读取和整体保存单例个人资料                     |
| GET         | `/ai-trending/repositories`、`/ai-trending/repositories/{id}`、`/ai-trending/repositories/{id}/snapshots` | 仓库与成功批次快照                             |
| PATCH       | `/ai-trending/repositories/{id}`                                                                          | 隐藏/恢复、精选、分类修正                      |
| GET         | `/ai-trending/runs`                                                                                       | 采集批次、错误与限流信息                       |
| POST        | `/ai-trending/ingestion-runs`                                                                             | Admin 请求当日批次；运行中或已成功返回现有记录 |
| GET         | `/publishing/jobs`、`/publishing/jobs/{id}`                                                               | 列表和详情；支持通过 generation 定位任务       |
| POST        | `/publishing/jobs/{id}/retry`                                                                             | Admin 重试仍为最新目标的失败任务，202          |
| GET, POST   | `/users`                                                                                                  | Admin 列表和创建账户                           |
| PATCH       | `/users/{id}`                                                                                             | Admin 停用、启用及分配内置角色                 |
| GET         | `/roles`、`/audit-logs`、`/system/health`                                                                 | Admin 读取内置角色、审计及系统健康             |

默认由 Editor/Admin 维护个人资料和分类标签。Author 仅可操作自己名下的文章与媒体，以及读取自己有权访问的任务。个人资料、公开分类标签和 AI 榜单展示设置保存成功后，响应包含 `publish_job_id` 和 `desired_generation`，供后台单独显示上线状态。

### 5.5 Internal API

内部接口仅在 Worker 与 API 分离部署且确有需要时启用。优先让 Worker 直接调用 application use case 和共享数据库，而不是为内部调用无谓增加 HTTP hop。

若生产入口独立于 Axum，入口使用服务身份调用 `GET /api/v1/internal/site/active` 解析 `active_generation` 和产物位置；不将产物位置直接透传给访客。不使用 HTTP 也可由同进程的 Axum handler 解析。Worker 的领任务、续租和发布确认一期直接走共享用例，不暴露额外的完成接口。

## 6. 核心 DTO 示例

### 6.1 创建文章与保存修订

```json
{
  "slug": "building-with-axum",
  "title": "Building with Axum",
  "excerpt": "A practical introduction.",
  "markdown_body": "# Introduction\n...",
  "category_ids": [],
  "tag_ids": [],
  "cover_media_id": null,
  "is_featured": false,
  "seo_title": "Building with Axum",
  "seo_description": "..."
}
```

创建返回 201，含 `post_id`、`revision_id`、`revision_no`。`POST /posts/{id}/revisions` 采用同样的完整修订内容，去掉 `slug`，新增必填 `expected_revision_id`。事务中校验文章权限、锁定文章行并比较当前指针，再追加 revision 和更新指针；不一致返回 `409 revision_conflict`（`details` 提供实际 `current_revision_id`），不创建修订。

`submit-review` 传 `revision_id`，创建 `pending` 审核并返回 201；重复提交返回 `409 review_already_submitted`。`reviews` 传 `revision_id`、`decision`（`approved` 或 `rejected`）及可选 `comment`，返回审核记录；不是待审核状态时返回 `409 review_not_pending`。文章详情返回 `current_revision_id`、可空的 `desired_published_revision_id` 和 `live_revision_id`、`publication_version` 及 `desired_generation`、`active_generation`。

### 6.2 发布与取消发布

```json
{
  "revision_id": "019535d9-3df7-79fb-b466-fa907fa17f9e",
  "expected_publication_version": 0
}
```

`unpublish` 只传 `expected_publication_version`。事务校验当前 `publication_version`、权限及指定 revision 的归属和审批结果；更新或清空期望公开 revision 和时间，同时增加 `publication_version`、创建 job 和 Outbox 事件。两类操作均返回 202，含 `publish_job_id`、`desired_generation`、`publication_version`，并不保证已上线。并发冲突为 `409 publication_conflict`，未批准为 `409 revision_not_approved`，无期望公开版本可撤下为 `409 already_unpublished`。

### 6.3 AI Trending 列表参数

`GET /api/v1/public/ai-trending?period=7d&sort=stars_growth&language=Rust&page=1&per_page=20`

- `period` 为 `7d`（默认）或 `30d`；`sort` 为 `stars_growth`（默认）、`forks_growth`、`stars` 或 `recently_updated`，榜单固定降序。
- 可用 `language`、`topic`、`category` 筛选，并使用标准分页。响应包括 `data_date`（成功批次 UTC 日期，尚无成功批次时为 `null`）、分页后的 `data` 及同样受筛选约束的 `featured`。`stars_growth`、`forks_growth` 可为负或 `null`。计算与排序见 §7.1。

### 6.4 媒体上传

`presign` 接受 `filename`、`declared_mime_type`、`size_bytes`，返回 201 和 `media_id`、`upload_url`、请求方法、签名要求的头及 `expires_at`。浏览器直传后调用 `complete`，不自行提交公开 URL；服务端校验后返回 200 和 `status: ready`。对象尚未就绪为 `409 upload_incomplete`，过期为 `409 upload_expired`，不符合校验规则为 `422 media_invalid`；被拒绝时重新申请上传。保存文章或个人资料时，引用未通过校验的媒体返回 `422 media_not_ready`。删除被引用的媒体返回 `409 media_in_use`。

## 7. 核心业务流程

### 7.1 AI 采集流程与排名口径

1. Scheduler 每天 02:00 UTC 创建该 UTC 日期的批次；手动触发也使用请求当日的 `(source_id, slot_date)`。通过唯一约束合并请求，仅可重试当日未成功批次，不补写逾期日期。租约过期的旧 Worker 不得提交结果。
2. Provider 从管理员配置的 topic 白名单发现公开 GitHub 仓库（初始可使用 `artificial-intelligence`、`machine-learning`、`llm`）。每个 topic 按 GitHub Stars 排序获取前 100 个候选，过滤 fork、归档及非公开仓库，按 GitHub ID 合并；按 Stars 降序、外部 ID 升序截取前 200 项。只采集当批候选，掉出收录范围的项目不在该批榜单及详情中出现，但保留历史样本。
3. 分页、ETag 缓存和限流重试只作用于采集层。必需的发现或详情请求未完成则整批失败；无 release 可以是有效空值，不把请求失败或缺失指标映射为 0。完整候选集可以为空；不能将请求失败当作空集。采集网络 I/O 不持有 DB 事务。
4. 仅在整批准备就绪后开启事务：校验租约与采集日，根据 `(source_id, external_id)` upsert 项目（保留人工字段），写入每个 `(run_id, repository_id)` 快照，将批次标记为 `succeeded`，并按 §7.2 创建新的站点目标。失败仅更新批次的状态、错误及统计，不提交业务数据或发布新批次。公开站点仍使用最近一次已上线的成功批次；下一次站点构建选择最新成功的 UTC 批次，与其他站点内容一同上线。
5. 以最新成功批次的 UTC 日期 `T` 为终点。对 `W=7/30` 天，在 `[T-W-2 天, T-W]`（均含边界）内选该项目日期最新的成功样本作为基线。`增长 = 终点计数 - 基线计数`，没有基线时为 `null`，保留负数；不跨批拼出终点。快照同时保留真实观测时间 `snapshot_at`。
6. 自然榜单只含该批次、未隐藏的项目；主排序按请求字段降序，增长为 `null` 者排在全部有值项之后；再按 Stars 降序、外部 ID 升序稳定排序。`recently_updated` 使用上游更新时间。精选为独立分区，其标记不参与自然榜单排序。历史接口只提供当前可见项目截至 `T` 的最近 90 天成功样本。页面显示 `T`；无成功批次时为暂无数据。

### 7.2 站点发布流程

1. **登记站点目标。** 文章发布或取消发布、公开分类标签修改、个人资料保存、AI 榜单展示调整和成功采集均调用 `enqueue_site_publish`。锁定 `site_state` 单例后递增 `desired_generation`（首次为 1），并在同一事务内保存业务变更、创建 `queued` 的 `publish_jobs` 与业务键为 generation 的 Outbox 事件。保存草稿、上传和审核不重建。初次部署以 `bootstrap` 触发，未上线前站点返回 503。
2. **领取与导出。** Worker 短事务领取任务，写入租约后提交。导出使用同一 `REPEATABLE READ` 视图，首先确认请求 generation 为该视图的 `desired_generation`，再一次性导出期望公开的 revision、分类标签、个人资料、最新成功 AI 批次、媒体 ID 与页面索引。输出带 `generation`、`schema_version` 和完整性摘要的不可变快照：仅含公开投影（不含账户、草稿、原始外部 payload 或内部 key），站点构建及 Public API 使用同一数据；文章 ID 与公开 revision 的映射也写入快照，供后台查询线上状态。
3. **构建候选产物。** Worker 将快照以只读文件交给 Astro，并将 HTML、sitemap、RSS 等输出上传至按 generation 和尝试隔离的私有产物位置。验证上传完成及产物清单后才能尝试上线。构建、上传均不修改生产路由；过期租约不能覆盖后续尝试的产物。
4. **原子切换。** 生产站点入口和 Public API 均以数据库 `site_state.active_generation` 为唯一线上指针。成功候选在短事务内锁定同一单例行，验证 `generation == desired_generation`、租约仍有效且产物已就绪，然后一次性写入对应快照与产物位置、推进 `active_generation`、标记当前任务为 `active` 与上一线上任务为 `retired`。若 `desired_generation` 已变，旧候选标记 `superseded`，不切流量；同一已激活任务重复确认是空操作。
5. **处理失败与重试。** 构建或校验失败保持上一指针，记录错误并把仍为最新目标的任务标为 `failed`；落后的未上线任务可直接合并为 `superseded`。Admin 只可把仍为最新目标的失败 generation 重新排队（否则 `409 publish_job_not_retryable`），不能激活旧站点产物。后续站点变更生成新 generation，并包含此前尚未上线的期望变更。
6. **访问与撤下。** 每次生产请求解析一次指针，并只提供该版本的内容；未命中的详情返回 404，不查询旧产物。Canonical URL 与公开媒体在切换后不能从过期缓存继续返回旧内容：一期 HTML、公共 API、媒体及 SEO 路由使用 `Cache-Control: no-store`；可按不可变产物键做内部缓存，哈希静态资产可长期缓存。上线后的撤下同时覆盖静态页、Public API、列表、sitemap 与 RSS。

## 8. Worker 与任务可靠性

- Outbox 至少一次投递；用 `FOR UPDATE SKIP LOCKED` 在短事务领取/确认通知，以 generation 去重。`publish_jobs` 和 `ai_ingestion_runs` 是持久任务；Worker 同时扫描可领取或可回收的记录，不能只依赖收到通知。
- 长任务领取、续租、抢占与完成都校验租约令牌，记录 attempts 与错误。崩溃或超时可回收，达到重试上限停止自动重试；构建和外部 I/O 不持有行锁。
- 对外部 API 使用 timeout、限流、指数退避和最大重试次数；同一采集日由唯一约束与租约协调，过期采集批次不补跑。

## 9. 安全要求

- 密码使用 Argon2id。
- 后台入口与浏览器访问的 Admin API 同源。Session Cookie 使用 HttpOnly、Secure、SameSite=Lax、Path=/ 和 host-only（不设置 Domain）；数据库只存 token hash。停用账号立即撤销该用户会话；权限检查使用当前账户状态与角色。
- 登录只接受 JSON 和配置的精确同源 Origin；其他管理写操作同时校验 Origin 和会话绑定的 `X-CSRF-Token`，由登录和 `auth/me` 返回。拒绝缺失或不匹配的 Origin；认证响应与非公开预览使用 `Cache-Control: no-store`。
- 在服务端每个管理用例执行产品 §3 的 RBAC 与文章/媒体归属检查，含嵌套修订读取、上传确认和媒体引用；前端隐藏按钮不作为授权。登录及 API 做限流，管理操作写审计。
- Markdown 渲染与预览使用同一套允许的标签、属性和 URL 协议；不要执行原始 HTML 或远程资源探测。上传校验与私有、公开读取边界见 §4.3。
- Secret 通过部署环境 Secret 管理，不提交到仓库。管理 API 不启用跨域凭证；对象存储直传 CORS 只允许后台 origin 和签名所需的方法、请求头。

## 10. 可观测性与部署

### 10.1 可观测性

- tracing + structured logging。
- 每个请求有 request_id。
- 记录 API latency、5xx、DB pool、Worker queue、采集成功率、构建耗时及变更从提交到上线的时延；仍有待上线变更超过 5 分钟时记录告警指标。
- 审计日志记录管理操作，但避免记录密码、Session token 和敏感凭据。

### 10.2 部署建议

第一阶段：

- Astro：只生成静态产物；一期由公开域入口（可由 Axum 路由实现）按线上指针从私有 S3/R2 读取对应静态文件。不要使用构建完自动切换生产域名的托管路径绕过 §7.2。
- React Admin：独立子域名静态托管，入口把同源 `/api/v1/admin` 代理到 Axum；公共域只暴露公共 API 和前台路由。Vite 开发代理遵守同源访问方式。
- Axum API 和 Worker 分别部署为 Docker 容器，使用托管或受控 PostgreSQL。私有对象存储用于上传临时对象、正式媒体、快照和站点产物，并按用途设置生命周期策略。
- 在生产上线前演练 PostgreSQL 和线上版本所依赖的对象恢复；退役产物按保留策略清理，禁止清理当前线上及待切换版本的依赖。Docker Compose 可用于本地开发和小规模部署。

初期不需要 Kubernetes 或微服务拆分。等到采集量、流量或团队规模形成真实瓶颈后再拆分。

## 11. 实施里程碑

1. Workspace、配置、迁移、健康检查、认证和 RBAC。
2. 媒体直传与确认、Markdown 修订、分类标签、审核、发布和取消发布。
3. Outbox、generation 快照、Astro 构建与生产指针切换；Public API 和 Admin 任务页面。
4. 个人资料维护与公开投影。
5. GitHub 采集、快照、趋势计算和后台运行记录。
6. 审计、监控、备份恢复和端到端测试。优先覆盖过期租约、旧构建晚完成、新草稿不影响线上、部分采集失败、取消发布后的 404 和媒体引用权限。
