# RESTful 接口设计规范

所有 `/api/*` 接口必须遵守本规范。本文规定路径与方法；响应正文、状态码细则和错误码见 [API 响应结构规范](api-response.md)。新增或修改接口时，先按本文确定资源与方法，再写 OpenAPI 与实现。

## 1. 资源与路径

1. **路径只由名词组成。** 路径表示资源，动作由 HTTP 方法表达。路径中不出现 `login`、`logout`、`refresh`、`info`、`create`、`update`、`delete`、`submit`、`publish`、`retry`、`complete` 之类的动词。
2. **集合用复数名词**：`/users`、`/audit-logs`。集合中的单个资源用 ID 定位：`/users/{id}`。
3. **单例资源用单数名词**：一个父资源下只有一份的资源，如 `/users/profile`、`/posts/{id}/publication`、`/users/me/password`。
4. **命名：** 路径段使用小写 kebab-case（`audit-logs`）；查询参数和 JSON 字段使用 `snake_case`。路径不带尾斜杠，不带扩展名。
5. **层级：** 只有从属关系才嵌套，最多嵌套到 `/{父集合}/{id}/{子集合}/{子id}`。能独立定位的资源不要嵌套。
6. **别名：** `me` 表示当前登录账户（`/users/me`），`current` 表示当前会话（`/sessions/current`）。别名是静态路径段，路由中优先于 `{id}` 匹配。
7. **版本：** 版本号放在路径前缀 `/api/v1`。不兼容变更才升级版本。

## 2. 方法语义

| 方法 | 作用 | 安全 | 幂等 | 成功状态 |
| --- | --- | --- | --- | --- |
| GET | 读取资源或集合，不带请求体 | 是 | 是 | 200 |
| POST | 在集合中创建资源，或创建一个没有独立 URI 的资源（如令牌对） | 否 | 否 | 201；异步受理为 202 |
| PUT | 整体替换资源；单例资源不存在时创建 | 否 | 是 | 200 返回资源，或 204 |
| PATCH | 部分更新资源，省略的字段保持原值 | 否 | 否 | 200 返回更新后的资源 |
| DELETE | 删除资源，不带请求体 | 否 | 是 | 204；返回撤回记录为 200，异步受理为 202 |

- 新建的资源有独立 URI 时，201 响应必须带 `Location` 响应头，值为该资源的路径。
- 路径存在但方法不支持时返回 405 `method_not_allowed`，不要把不支持的方法映射到其他行为。
- 过滤、排序、分页使用查询参数，例如 `GET /users?status=active&page=2`。不要用 `POST /users/search` 之类的路径。

## 3. 把动作建模为资源

需要表达动作时，按下面的顺序选择：

1. **改字段：** 动作只是改变资源状态时，用 PATCH 修改对应字段。例如停用账户为 `PATCH /users/{id}` 提交 `{"status": "disabled"}`。
2. **建子资源：** 动作会留下记录或需要单独追踪时，创建对应的子资源。例如保存文章修订使用 `POST /posts/{id}/revisions`。
3. **操作单例：** 动作是设置或撤销某个“唯一状态”时，对单例资源使用 PUT 和 DELETE。例如上线为 `PUT /posts/{id}/publication`，下线为 `DELETE /posts/{id}/publication`。

当前和规划中的对照：

| 动作 | 不使用 | 使用 |
| --- | --- | --- |
| 登录 | `POST /auth/login` | `POST /sessions` → 201 |
| 刷新令牌 | `POST /auth/refresh` | `POST /tokens` → 201 |
| 退出登录 | `POST /auth/logout` | `DELETE /sessions/current` → 204 |
| 读取当前账户 | `GET /users/info` | `GET /users/me` |
| 修改本人密码 | `POST /auth/password` | `PUT /users/me/password` → 204 |
| 上线、下线 | `POST /posts/{id}/publish`、`/unpublish` | `PUT`、`DELETE /posts/{id}/publication` → 200 |
| 上传文件 | `POST /media/presign`、`/media/{id}/complete` | `POST /media` → 201 |

### 删除文章

`DELETE /api/v1/admin/posts/{id}` 要求 `posts.manage`，查询参数为 `expected_revision_id` 和 `expected_publication_version`。文章须已撤回或从未发布，且未归属系列。成功返回 204，并在同一事务记录 `post.delete` 审计日志。

### 系列资源

以下路径以 `/api/v1/admin` 为前缀。读取、筛选、编辑均要求 `series.manage`，公开转换还要求 `posts.publish`。

| 资源 | 方法与结果 |
| --- | --- |
| `/series` | GET 按关键字、主分类、公开设置、连载状态分页排序。POST 创建草稿，返回 201 与 Location。 |
| `/series/{id}` | GET 返回资料、目录、两个版本、线上状态与成员统计。PATCH 按编辑版本部分更新，可空字段显式传 null 表示清除。DELETE 通过查询传入两个版本，只可删除未公开的系列。 |
| `/series/{id}/outline` | PUT 同时替换篇章和根条目，两者均必填。已有篇章使用原 ID，新增篇章由服务端分配 ID。最多 50 个篇章、200 篇文章，正文上限 1 MiB。 |
| `/series/{id}/items` | POST 按编辑版本追加至指定篇章或根目录末尾，返回 201、Location、条目、新编辑版本与可空发布目标。 |
| `/series/{id}/items/{post_id}` | DELETE 查询参数携带编辑版本，成功返回 204。 |
| `/series/{id}/publication` | PUT 在正文传发布版本，DELETE 在查询参数传发布版本。成功均返回 200 和已提交的发布记录。 |

元数据 PATCH 与目录 PUT 的 200 写响应在 `data` 内返回 `series` 与可空的 `publication`。编辑已有公开目标的系列时，实际变更与一次站点任务原子提交。无变化保存沿用原编辑版本。文章列表与分类统计支持 `series_id` 和 `in_series`，其中 `series_id` 与 `in_series=false` 互斥。文章响应中的 `series` 仅向系列管理者提供。

## 4. 例外

运维端点 `/health/live` 和 `/metrics` 沿用 Kubernetes 和 Prometheus 的惯例路径，不加 `/api` 前缀，也不套用响应信封。

## 5. 自动校验

`apps/api/tests/openapi.rs` 中的 `paths_follow_restful_conventions` 检查 OpenAPI 中的每条路径：路径段必须是小写 kebab-case 或 `{param}`，不能是常见动词；DELETE 声明 204，返回撤回记录时声明 200；GET 不能有请求体；创建账户的 201 必须声明 `Location`。`openapi_document_matches_official_schema` 同时保证 OpenAPI 与实际路由一致。
