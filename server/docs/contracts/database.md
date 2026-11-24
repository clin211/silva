# 数据库实现契约

依据：`docs/03-backend-technical.md` §3、§4、§9、§11（版本 1.2），已逐节读取。此切片交付 SQLx migrations 和 PostgreSQL 验证 SQL；不实现后续业务用例。

## 基础不变量

- PostgreSQL 原生 `uuid`；先创建 `is_uuid_v7(input_id uuid) RETURNS boolean`，声明 `IMMUTABLE STRICT PARALLEL SAFE`。检查文本第 15 位为 `7`、第 20 位属于 `8/9/a/b`。不依赖数据库 UUIDv7 生成函数。
- 全部业务实体 ID、无外键的 `outbox_events.aggregate_id`、`audit_logs.resource_id` 和 `audit_logs.request_id`（迁移 `20261123000001` 追加，可为 NULL）由 CHECK 校验 UUIDv7；普通/复合外键引用受校验实体。租约令牌是令牌，不强制 UUIDv7。`generation`/`revision_no`/slug/角色 key/外部来源 ID 不转为 UUID。
- 每张业务表及每个未删除用户字段必须有非空 `COMMENT ON TABLE/COLUMN`；不检查 PostgreSQL 系统表、`_sqlx_migrations`。所有字段的注释同迁移创建。CI 通过 `obj_description`、`col_description` 验证并输出具体缺失对象。
- 所有 FK 写出删除策略。原文已有策略原样保留；原文未指定者统一显式 `ON DELETE NO ACTION`，保持 PostgreSQL 默认语义、避免擅自删除作者/审计/发布引用。两个 post revision 复合 FK 保持 `DEFERRABLE INITIALLY DEFERRED`，使用 `ON DELETE NO ACTION` 以允许事务内先预分配 post/revision ID 后写入。
- `site_state` 在 schema migration 初始化唯一 `singleton=true` 行；期望与线上 generation 初值 NULL，不能伪造已上线。
- 账户密码仅 Argon2id hash、session 仅当前 refresh token 的 SHA-256 hash（迁移 `20261124000001` 更新 `sessions` 注释）；角色和权限由 Rust seed 通过 `shared::ids` 分配 UUIDv7，SQL 不写固定业务实体 ID。
- 媒体（迁移 `20261124000002`）：`media_assets` 增加客户端声明的 `declared_mime_type`、`declared_size_bytes`，拒绝原因 `rejection_reason`、拒绝时间 `rejected_at`（已拒绝记录的保留期按它计算），Worker 清理失败时间 `purge_attempted_at`（失败的记录排到后续批次末尾），以及由触发器维护的 `updated_at`。CHECK 约束限定声明与实际类型为 JPEG、PNG、WebP，大小不超过 10 MiB，`ready` 必须有尺寸，`rejection_reason` 与 `rejected_at` 当且仅当 `rejected` 时有值。对象存储键 `upload_key`（`uploads/{id}`）与 `storage_key`（`media/{id}`）只在服务端使用，不出现在 API 中。删除媒体时锁定媒体行，并检查 `post_revisions.cover_media_id`、`post_revision_media` 和 `profiles.avatar_media_id` 中的引用；这三列补充了索引。
- 项目展览与工具导航（迁移 `20261124000003`）：产品已取消这两项功能，删除 `projects`、`project_technologies`、`project_links`、`tools`，它们对媒体的引用随之移除。引用媒体的表只剩 `post_revisions`、`post_revision_media` 和 `profiles`。
- schema 不替代业务授权：账号停用的会话撤销、动态读取角色权限、JWT 校验与 refresh token 轮换、媒体就绪及归属、只追加 revision、generation 原子切换和租约 fencing 由对应 Rust 用例负责。

## 实施约束与待确认项

- 当前 DDL 是完整逻辑基线，逐字段保留下方 SQL，包括 date 类型的业务日期和 JSONB 外部数据/配置；不擅自给原文缺少时间戳的纯字典和关联表加字段。
- 原文 `users.status` 缺少 CHECK；身份里程碑将补 `active/disabled` CHECK，并加 case-insensitive 邮箱唯一索引，确保大小写重复登录标识不能出现。主线程已确认状态值。
- 原文没有 `updated_at` 自动维护机制；建议给已具有 `updated_at` 的表统一 trigger，每次实际 UPDATE 维护数据库更新时间，主线程已确认实施。
- 原文后续业务约束（例如 revision 不可变、AI run 与 repository 同源、profile 单例）目前有应用层说明但缺少数据库 CHECK/trigger；此切片不发明额外字段或任务协议。

## 验证锚点

1. 空 PostgreSQL 数据库执行全部 SQLx migration；再次 migrate 无操作，校验校验和。已有数据库追加执行清理迁移；已执行的历史迁移保留原版本与校验和。
2. `tests/sql/schema_invariants.sql` 在事务内检查当前 27 张业务表、21 个 UUIDv7 实体的主键 CHECK 覆盖及全部业务表/字段注释；缺失或已取消的目录、项目展览与工具导航表仍存在时，输出对象名并使执行失败。同一脚本验证媒体状态约束（`ready` 缺尺寸、`rejected` 缺原因、`pending` 带原因、不允许的声明类型均被拒绝）和 `media_assets.updated_at` 触发器。
3. 验证 `is_uuid_v7` 接受合法 v7、拒绝 v4 及版本位为 7 但变体位错误的 UUID；逐个实体表通过实际 INSERT 或事务更新现有主键验证 v4/错误变体被 CHECK 拒绝。
4. 验证实际 `users` v7 插入成功、v4 和错误变体失败；无外键 UUID 引用（outbox aggregate、audit resource）也拒绝非法 UUID。
5. 检查全部 FK 的删除策略、两个 post revision 外键的延迟属性、唯一约束、site_state 单例和未上线初始状态。
6. PostgreSQL 客户端采用 `ON_ERROR_STOP=1`；验证 SQL 由 CLI 使用 SQLx 在外层事务执行并 rollback，不污染开发数据库；SQL 文件不包含事务命令和 psql 元命令。CLI 通过 `set_config` 设置 `silva.test_id` 和 `silva.test_other_id`，两者均由 `shared::ids` 生成不同的 UUIDv7。

## 当前基线 DDL（源文逐字提取）

```sql
CREATE FUNCTION is_uuid_v7(input_id uuid) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
AS 'SELECT substr(input_id::text, 15, 1) = ''7''
       AND substr(input_id::text, 20, 1) IN (''8'', ''9'', ''a'', ''b'')';
```

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

COMMENT ON TABLE sessions IS '后台登录会话，每次登录创建一条；access token 与 refresh token 通过会话 ID 关联，删除该行即撤销其全部令牌。';
COMMENT ON COLUMN sessions.id IS '该记录的 UUIDv7 主键。';
COMMENT ON COLUMN sessions.user_id IS '会话所属的后台用户 ID';
COMMENT ON COLUMN sessions.token_hash IS '当前有效 refresh token 的 SHA-256 哈希，每次刷新轮换；不保存令牌原文';
COMMENT ON COLUMN sessions.expires_at IS '当前 refresh token 的过期时间，即会话失效时间；每次刷新按 refresh token 有效期重新计算';
COMMENT ON COLUMN sessions.created_at IS '该记录首次写入本系统的时间。';
COMMENT ON COLUMN sessions.last_seen_at IS '最近一次记录的会话活动时间（鉴权最多每 5 分钟更新一次，刷新令牌时更新），未记录时为 NULL';
```

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
