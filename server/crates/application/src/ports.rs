use async_trait::async_trait;
use chrono::{DateTime, Utc};
use silva_domain::{
    content::{
        Category, Heading, Post, PostFilter, PostSummary, Review, ReviewDecision, Revision,
        RevisionContent, RevisionSummary, Tag, TaxonomyInput,
    },
    identity::{AuditLog, AuditLogFilter, Principal, Role, User},
    media::{Media, MediaStatus, RejectionReason, VerifiedImage},
    site::{
        PublicationAccepted, PublishJob, PublishJobFilter, SiteTarget, SnapshotCategory,
        SnapshotImage, SnapshotLink, SnapshotMedia, SnapshotPost, SnapshotSkill, SnapshotTag,
    },
};
use silva_shared::{
    error::Result,
    pagination::{Page, PageRequest},
};
use std::collections::HashMap;
use std::net::IpAddr;
use uuid::Uuid;

/// 写入审计的请求来源信息；由 HTTP 边界提取，CLI 等内部操作使用默认值。
#[derive(Clone, Debug, Default)]
pub struct RequestMeta {
    pub ip_address: Option<IpAddr>,
    pub user_agent: Option<String>,
    /// 当前 HTTP 请求的 UUIDv7，与响应头 `X-Request-Id` 一致；CLI 等非 HTTP 操作为 None。
    pub request_id: Option<Uuid>,
}

/// 按规范化邮箱查到的登录账户，包含停用账户，以便审计区分失败原因。
pub struct LoginRecord {
    pub id: Uuid,
    pub password_hash: String,
    /// 账户是否为启用状态；停用账户不得登录。
    pub active: bool,
}

/// 登录失败的审计原因，写入 `auth.login_failed` 审计的 `metadata.reason`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginFailure {
    /// 账户不存在或密码错误。
    InvalidCredentials,
    /// 密码正确但账户已停用。
    AccountDisabled,
    /// 该邮箱处于连续失败锁定期，未校验密码。
    Locked,
}

impl LoginFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCredentials => "invalid_credentials",
            Self::AccountDisabled => "account_disabled",
            Self::Locked => "locked",
        }
    }
}

pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub roles: Vec<String>,
}

/// 账户列表的筛选条件；由 `UserFilter::new` 规范化并校验，多个条件按 AND 组合。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserFilter {
    /// 已去首尾空白的非空关键字，按 email 或 display_name 不区分大小写子串匹配。
    pub q: Option<String>,
    /// 内置角色键。
    pub role: Option<String>,
    /// 账户状态。
    pub status: Option<String>,
}

pub struct UserChanges {
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub roles: Option<Vec<String>>,
    /// 管理员重置后的 Argon2id 哈希；设置时撤销该账户全部会话。
    pub password_hash: Option<String>,
}

#[async_trait]
pub trait IdentityRepository: Send + Sync {
    /// 按规范化邮箱查找账户（含停用账户）。
    async fn find_login(&self, email: &str) -> Result<Option<LoginRecord>>;
    /// 写入 `auth.login_failed` 审计；`user_id` 仅在账户存在时提供，不记录尝试的邮箱或密码。
    async fn record_login_failure(
        &self,
        user_id: Option<Uuid>,
        reason: LoginFailure,
        meta: &RequestMeta,
    ) -> Result<()>;
    /// 锁定有效账户，在同一事务中创建会话、更新登录时间并写入审计。
    /// `token_hash` 为当前有效 refresh token 的哈希，`expires_at` 为其过期时间。
    async fn create_session(&self, session: &NewSession<'_>, meta: &RequestMeta) -> Result<()>;
    /// 按会话 ID 读取未过期、属于该账户且账户仍启用的会话，并重新读取账户状态和角色权限，
    /// 不缓存登录时的授权结果；不满足任一条件返回 `Unauthorized`。
    async fn principal(&self, session_id: Uuid, user_id: Uuid) -> Result<Principal>;
    /// 轮换会话的 refresh token：在同一事务中锁定会话行，哈希一致时替换为新哈希与过期时间；
    /// 哈希不一致视为重放，删除该会话并写入 `auth.refresh_reuse` 审计。
    async fn rotate_session(
        &self,
        rotation: &SessionRotation<'_>,
        meta: &RequestMeta,
    ) -> Result<RotationOutcome>;
    async fn revoke_session(&self, principal: &Principal, meta: &RequestMeta) -> Result<()>;
    /// 读取启用账户的密码哈希，用于修改密码前校验当前密码。
    async fn password_hash(&self, user_id: Uuid) -> Result<Option<String>>;
    /// 在同一事务中替换密码哈希、撤销该账户除当前会话外的全部会话并写入审计。
    async fn change_password(
        &self,
        principal: &Principal,
        password_hash: String,
        meta: &RequestMeta,
    ) -> Result<()>;
    /// 按筛选条件分页查询账户；total 为筛选后的数量。
    async fn list_users(&self, page: &PageRequest, filter: &UserFilter) -> Result<Page<User>>;
    /// 读取单个账户；不存在时返回 `NotFound`。
    async fn find_user(&self, id: Uuid) -> Result<User>;
    async fn create_user(
        &self,
        actor: Option<Uuid>,
        user: NewUser,
        meta: &RequestMeta,
    ) -> Result<User>;
    /// 在同一事务中锁定账户、替换角色分配，并撤销已停用账户的会话。
    /// 修改后若不再有启用状态的 admin，返回 `Conflict(Conflict::LastAdminRequired)` 并回滚。
    async fn update_user(
        &self,
        actor: Uuid,
        id: Uuid,
        changes: UserChanges,
        meta: &RequestMeta,
    ) -> Result<User>;
    async fn roles(&self) -> Result<Vec<Role>>;
    /// 按筛选条件分页查询审计记录，`total` 为过滤后的总数。
    async fn audit_logs(
        &self,
        page: &PageRequest,
        filter: &AuditLogFilter,
    ) -> Result<Page<AuditLog>>;
    async fn audit_log(&self, id: Uuid) -> Result<Option<AuditLog>>;
}

#[async_trait]
pub trait Credentials: Send + Sync {
    async fn hash_password(&self, password: String) -> Result<String>;
    /// 账户不存在时也执行一次虚拟 Argon2 校验，减少通过耗时差异枚举账户的风险。
    async fn verify_password(
        &self,
        password: String,
        password_hash: Option<String>,
    ) -> Result<bool>;
    /// 计算令牌的不可逆哈希；数据库只保存哈希，不保存令牌原文。
    fn token_hash(&self, token: &str) -> String;
}

/// 新建会话的持久化数据。
pub struct NewSession<'a> {
    pub id: Uuid,
    pub user_id: Uuid,
    /// 当前有效 refresh token 的哈希。
    pub token_hash: &'a str,
    /// refresh token 的过期时间，即会话失效时间。
    pub expires_at: DateTime<Utc>,
}

/// 一次 refresh token 轮换请求。
pub struct SessionRotation<'a> {
    pub session_id: Uuid,
    pub user_id: Uuid,
    /// 客户端提交的 refresh token 哈希。
    pub presented_hash: &'a str,
    /// 新签发 refresh token 的哈希。
    pub new_hash: &'a str,
    pub expires_at: DateTime<Utc>,
}

/// refresh token 轮换结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationOutcome {
    /// 已替换为新 refresh token。
    Rotated,
    /// 提交的 refresh token 已被轮换过，判定为重放；会话已删除并写入审计。
    Reused,
    /// 会话不存在、已过期、不属于该账户或账户已停用。
    Rejected,
}

/// 令牌种类，对应 JWT 声明 `typ`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Access,
    Refresh,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
        }
    }
}

/// 验证通过的令牌声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenClaims {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub kind: TokenKind,
}

/// 令牌验证失败的原因；只有签名与声明均有效、仅时间过期时才是 `Expired`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRejection {
    Expired,
    Invalid,
}

/// 一次签发的 access token 与 refresh token。
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// access token 的过期时间（UTC，精确到秒，与其 `exp` 声明一致）。
    pub access_expires_at: DateTime<Utc>,
    /// refresh token 的过期时间（UTC，精确到秒，与其 `exp` 声明一致），同时写入会话的 `expires_at`。
    pub refresh_expires_at: DateTime<Utc>,
}

/// 令牌的签发与验证。实现必须固定签名算法，拒绝其他算法和未签名令牌。
pub trait TokenService: Send + Sync {
    /// 为指定会话签发一对新令牌，每个令牌带独立的 `jti`。
    fn issue(&self, user_id: Uuid, session_id: Uuid) -> Result<IssuedTokens>;
    /// 校验签名、签发方、受众、种类与过期时间。
    fn verify(
        &self,
        token: &str,
        kind: TokenKind,
    ) -> std::result::Result<TokenClaims, TokenRejection>;
}

/// 新建媒体记录的持久化数据。
pub struct NewMedia {
    pub id: Uuid,
    pub upload_key: String,
    pub original_filename: String,
    pub declared_mime_type: String,
    pub declared_size_bytes: u64,
    pub upload_expires_at: DateTime<Utc>,
    pub uploaded_by: Uuid,
}

/// 媒体记录及其内部对象键；对象键只在服务端使用，不对外返回。
#[derive(Clone, Debug)]
pub struct MediaRecord {
    pub media: Media,
    pub upload_key: String,
    pub storage_key: Option<String>,
}

/// 媒体列表筛选条件，多个条件按 AND 组合。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaFilter {
    pub status: Option<MediaStatus>,
    /// 只返回该账户上传的媒体；无 `media.manage` 权限时由用例强制设为当前账户。
    pub uploaded_by: Option<Uuid>,
}

/// 媒体变更的操作者与请求来源，用于审计。
pub struct MediaActor<'a> {
    pub user_id: Uuid,
    pub meta: &'a RequestMeta,
}

/// Worker 清理的过期或已拒绝媒体：删除记录前需清理的临时对象键。
#[derive(Clone, Debug)]
pub struct StaleMedia {
    pub id: Uuid,
    pub upload_key: String,
}

#[async_trait]
pub trait MediaRepository: Send + Sync {
    /// 创建 `pending` 记录并写入 `media.create` 审计。
    async fn create(&self, media: &NewMedia, meta: &RequestMeta) -> Result<Media>;
    async fn find(&self, id: Uuid) -> Result<Option<MediaRecord>>;
    /// 按筛选条件分页，按 `created_at DESC, id DESC` 排序。
    async fn list(&self, page: &PageRequest, filter: &MediaFilter) -> Result<Page<Media>>;
    /// 仅当记录仍为 `pending` 时标记为 `ready` 并写入审计；已不是 `pending` 时返回 None。
    async fn mark_ready(
        &self,
        id: Uuid,
        storage_key: &str,
        image: &VerifiedImage,
        actor: &MediaActor<'_>,
    ) -> Result<Option<Media>>;
    /// 仅当记录仍为 `pending` 时标记为 `rejected` 并写入审计；已不是 `pending` 时返回 None。
    async fn mark_rejected(
        &self,
        id: Uuid,
        reason: RejectionReason,
        actor: &MediaActor<'_>,
    ) -> Result<Option<Media>>;
    /// 替换替代文本（None 表示清除）并写入审计；记录不存在时返回 `NotFound`。
    async fn update_alt_text(
        &self,
        id: Uuid,
        alt_text: Option<&str>,
        actor: &MediaActor<'_>,
    ) -> Result<Media>;
    /// 锁定并删除记录、写入审计，返回删除前的对象键。仍被文章修订或个人资料引用时
    /// 返回 `Conflict(MediaInUse)` 并回滚。
    async fn delete(&self, id: Uuid, actor: &MediaActor<'_>) -> Result<MediaRecord>;
    /// 返回最多 `limit` 条可清理的记录：直传截止时间早于 `expired_before` 的 `pending`，
    /// 以及被拒绝时间早于 `rejected_before` 的 `rejected`。保留期按拒绝时间计算，
    /// 修改替代文本等后续更新不会延后清理。仍被引用的记录不返回；从未清理失败的记录在前，
    /// 清理失败过的按最近失败时间排在后面，持续失败的记录不会挡住其他记录。
    async fn stale(
        &self,
        expired_before: DateTime<Utc>,
        rejected_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StaleMedia>>;
    /// 删除仍处于 `pending` 或 `rejected` 的记录；状态已变化或仍被引用时不删除并返回 false。
    async fn purge(&self, id: Uuid) -> Result<bool>;
    /// 记录一次清理失败，使该记录排到后续批次的末尾。
    async fn defer_purge(&self, id: Uuid) -> Result<()>;
}

/// 有大小上限的对象读取结果。
#[derive(Debug, PartialEq, Eq)]
pub enum StoredObject {
    Missing,
    /// 对象存在但超过读取上限，未下载内容。
    TooLarge {
        size: u64,
    },
    Bytes(Vec<u8>),
}

/// 对象内容的字节流。
pub type ByteStream =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = std::io::Result<bytes::Bytes>> + Send>>;

/// 有大小上限的流式读取结果。
pub enum StreamedObject {
    Missing,
    /// 对象存在但超过读取上限，未开始传输。
    TooLarge {
        size: u64,
    },
    Stream {
        size: u64,
        body: ByteStream,
    },
}

/// 条件写入的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Created,
    /// 同名对象已存在，未覆盖。
    AlreadyExists,
}

/// 私有对象存储。实现须保证 bucket 不公开，键由服务端生成。
#[async_trait]
pub trait ObjectStorage: Send + Sync {
    /// 使用声明长度流式写入新对象，同名对象不可覆盖。
    async fn put_stream(
        &self,
        key: &str,
        content_type: &str,
        length: u64,
        body: ByteStream,
    ) -> Result<()>;
    /// 签发仅可读取已校验对象的短期地址。
    async fn view_url(&self, key: &str, ttl: std::time::Duration) -> Result<String>;
    /// 读取对象，超过 `max_bytes` 时不下载内容。
    async fn read(&self, key: &str, max_bytes: u64) -> Result<StoredObject>;
    /// 以字节流读取对象，不把整个对象读入内存；超过 `max_bytes` 时不传输内容。
    async fn stream(&self, key: &str, max_bytes: u64) -> Result<StreamedObject>;
    /// 仅在键不存在时写入，已存在时不覆盖。
    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<PutOutcome>;
    /// 删除对象；对象不存在视为成功。
    async fn delete(&self, key: &str) -> Result<()>;
}

/// 按实际字节识别并完整解码图片。
#[async_trait]
pub trait ImageInspector: Send + Sync {
    /// 返回校验通过的图片信息，或拒绝原因；只有基础设施故障才返回 `Err`。
    async fn inspect(
        &self,
        bytes: Vec<u8>,
    ) -> Result<(Vec<u8>, std::result::Result<VerifiedImage, RejectionReason>)>;
}

/// 图片尺寸，渲染正文图片时写入 `width`、`height`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageSize {
    pub width: i32,
    pub height: i32,
}

/// Markdown 渲染结果：消毒后的 HTML 与目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedMarkdown {
    pub html: String,
    pub headings: Vec<Heading>,
}

/// Markdown 渲染与消毒。
pub trait MarkdownRenderer: Send + Sync {
    /// 提取正文引用的媒体 ID（去重、保持顺序）。图片不是 `media:<UUIDv7>` 时返回 `field` 上的校验错误。
    fn media_references(&self, field: &'static str, markdown: &str) -> Result<Vec<Uuid>>;
    /// 渲染并消毒；`media` 提供引用媒体的尺寸，缺失的媒体只保留说明文字。
    fn render(&self, markdown: &str, media: &HashMap<Uuid, ImageSize>) -> RenderedMarkdown;
}

/// 修订可引用的媒体信息。
#[derive(Clone, Debug)]
pub struct MediaReference {
    pub id: Uuid,
    pub status: MediaStatus,
    pub uploaded_by: Option<Uuid>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// 待写入的修订。`revision_no` 由仓储在锁定文章后分配。
pub struct NewRevision {
    pub id: Uuid,
    pub content: RevisionContent,
    pub rendered: RenderedMarkdown,
    /// 正文引用的媒体（不含封面）。
    pub body_media_ids: Vec<Uuid>,
    pub created_by: Uuid,
}

/// 新文章及其首条修订。
pub struct NewPost {
    pub id: Uuid,
    pub slug: String,
    pub author_id: Uuid,
    pub revision: NewRevision,
}

/// 用于授权判断的文章摘要。
#[derive(Clone, Debug)]
pub struct PostAccess {
    pub id: Uuid,
    pub author_id: Option<Uuid>,
    pub current_revision_id: Uuid,
}

/// 保存修订的结果。
pub enum SavedRevision {
    /// 创建了新修订。
    Created(Revision),
    /// 内容与当前编辑版本相同，未创建新修订，返回当前编辑版本。
    Unchanged(Revision),
}

/// 内容操作的执行者与请求来源，用于审计。
pub struct ContentActor<'a> {
    pub user_id: Uuid,
    pub meta: &'a RequestMeta,
}

#[async_trait]
pub trait ContentRepository: Send + Sync {
    /// 读取媒体的状态、归属与尺寸；不存在的 ID 不返回。
    async fn media_references(&self, ids: &[Uuid]) -> Result<Vec<MediaReference>>;
    /// 修订引用的全部媒体（封面与正文）。
    async fn revision_media(&self, revision_id: Uuid) -> Result<Vec<Uuid>>;
    /// 返回不存在的分类与标签 ID。
    async fn missing_taxonomy(
        &self,
        category_ids: &[Uuid],
        tag_ids: &[Uuid],
    ) -> Result<(Vec<Uuid>, Vec<Uuid>)>;
    /// 在一个事务内写入文章与首条修订；slug 已被使用时返回 `Conflict(SlugInUse)`。
    async fn create_post(&self, post: NewPost, actor: &ContentActor<'_>) -> Result<Post>;
    async fn post_access(&self, id: Uuid) -> Result<Option<PostAccess>>;
    async fn post(&self, id: Uuid) -> Result<Option<Post>>;
    /// 按筛选条件分页，按 `updated_at DESC, id DESC` 排序。
    async fn list_posts(
        &self,
        page: &PageRequest,
        filter: &PostFilter,
    ) -> Result<Page<PostSummary>>;
    /// 锁定文章并比较当前编辑版本：不一致返回 `Conflict(RevisionConflict)`；
    /// 内容与当前版本相同时不创建修订。
    async fn create_revision(
        &self,
        post_id: Uuid,
        expected_revision_id: Uuid,
        revision: NewRevision,
        actor: &ContentActor<'_>,
    ) -> Result<SavedRevision>;
    /// 修改 slug；已锁定返回 `Conflict(SlugLocked)`，被占用返回 `Conflict(SlugInUse)`。
    async fn update_slug(
        &self,
        post_id: Uuid,
        slug: &str,
        actor: &ContentActor<'_>,
    ) -> Result<Post>;
    async fn revisions(&self, post_id: Uuid, page: &PageRequest) -> Result<Page<RevisionSummary>>;
    async fn revision(&self, post_id: Uuid, revision_id: Uuid) -> Result<Option<Revision>>;
    /// 为修订创建待审核记录；已提交过返回 `Conflict(ReviewAlreadySubmitted)`。
    async fn create_review(
        &self,
        post_id: Uuid,
        revision_id: Uuid,
        actor: &ContentActor<'_>,
    ) -> Result<Review>;
    /// 对待审核记录作出决定；不是待审核返回 `Conflict(ReviewNotPending)`。
    async fn decide_review(
        &self,
        post_id: Uuid,
        review_id: Uuid,
        decision: ReviewDecision,
        comment: Option<String>,
        actor: &ContentActor<'_>,
    ) -> Result<Review>;
    async fn reviews(&self, post_id: Uuid, page: &PageRequest) -> Result<Page<Review>>;
    async fn review(&self, post_id: Uuid, review_id: Uuid) -> Result<Option<Review>>;
    /// 设置期望公开修订并登记站点目标。版本不一致返回 `Conflict(PublicationConflict)`；
    /// 修订未获批准返回 `Conflict(RevisionNotApproved)`；修订不属于该文章返回 `NotFound`。
    async fn publish(
        &self,
        post_id: Uuid,
        revision_id: Uuid,
        expected_version: i64,
        actor: &ContentActor<'_>,
    ) -> Result<PublicationAccepted>;
    /// 清空期望公开修订并登记站点目标；没有发布目标返回 `Conflict(AlreadyUnpublished)`。
    async fn unpublish(
        &self,
        post_id: Uuid,
        expected_version: i64,
        actor: &ContentActor<'_>,
    ) -> Result<PublicationAccepted>;
}

/// 分类或标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaxonomyKind {
    Category,
    Tag,
}

impl TaxonomyKind {
    /// 审计的资源类型。
    pub fn resource_type(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Tag => "tag",
        }
    }
}

/// 分类或标签的通用表示，序列化为对应资源本身。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(untagged)]
pub enum TaxonomyItem {
    Category(Category),
    Tag(Tag),
}

#[async_trait]
pub trait TaxonomyRepository: Send + Sync {
    /// 全部条目，按名称、ID 排序。
    async fn list(&self, kind: TaxonomyKind) -> Result<Vec<TaxonomyItem>>;
    async fn find(&self, kind: TaxonomyKind, id: Uuid) -> Result<Option<TaxonomyItem>>;
    /// slug 已被使用时返回 `Conflict(SlugInUse)`。
    async fn create(
        &self,
        kind: TaxonomyKind,
        id: Uuid,
        slug: &str,
        input: &TaxonomyInput,
        actor: &ContentActor<'_>,
    ) -> Result<TaxonomyItem>;
    /// 更新名称与说明；条目被期望公开的修订引用时在同一事务登记站点目标。
    async fn update(
        &self,
        kind: TaxonomyKind,
        id: Uuid,
        input: &TaxonomyInput,
        actor: &ContentActor<'_>,
    ) -> Result<(TaxonomyItem, Option<SiteTarget>)>;
    /// 有修订引用时返回 `Conflict(TaxonomyInUse)`。
    async fn delete(&self, kind: TaxonomyKind, id: Uuid, actor: &ContentActor<'_>) -> Result<()>;
}

/// Worker 领取的构建任务。
#[derive(Clone, Debug)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub generation: i64,
    /// 本次构建的序号（从 1 开始），用于隔离产物位置。
    pub attempt: i32,
    pub lease_token: Uuid,
}

/// 导出的站点数据；个人资料的 Markdown 由用例渲染。
pub struct ExportedSite {
    pub posts: Vec<SnapshotPost>,
    pub categories: Vec<SnapshotCategory>,
    pub tags: Vec<SnapshotTag>,
    pub profile: Option<ExportedProfile>,
    pub media: Vec<SnapshotMedia>,
}

/// 导出的个人资料原始内容。
pub struct ExportedProfile {
    pub display_name: String,
    pub headline: Option<String>,
    pub bio_md: Option<String>,
    pub location: Option<String>,
    pub avatar: Option<SnapshotImage>,
    pub experiences: Vec<ExportedExperience>,
    pub skills: Vec<SnapshotSkill>,
    pub links: Vec<SnapshotLink>,
}

pub struct ExportedExperience {
    pub organization: String,
    pub role: String,
    pub description_md: Option<String>,
    pub started_on: Option<chrono::NaiveDate>,
    pub ended_on: Option<chrono::NaiveDate>,
}

/// 线上切换的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// 已成为线上版本。
    Activated,
    /// 期间已有更新的目标，任务标记为 superseded，未切换。
    Superseded,
    /// 租约已失效（超时被回收或已由其他 Worker 接手），未做任何修改。
    LeaseLost,
}

/// 当前线上站点的位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSite {
    pub generation: i64,
    pub manifest_key: String,
    pub artifact_prefix: String,
}

#[async_trait]
pub trait PublishingRepository: Send + Sync {
    async fn jobs(&self, page: &PageRequest, filter: &PublishJobFilter)
    -> Result<Page<PublishJob>>;
    async fn job(&self, id: Uuid) -> Result<Option<PublishJob>>;
    /// 登记一个新的站点目标（全站重建）。
    async fn request_build(
        &self,
        trigger_type: &str,
        actor: Option<&ContentActor<'_>>,
    ) -> Result<SiteTarget>;
    /// 把仍为最新目标的失败任务重新排队，再给一次构建机会（`attempts` 不清零，再次失败即为
    /// failed）；否则返回 `Conflict(PublishJobNotRetryable)`。
    async fn retry(&self, id: Uuid, actor: &ContentActor<'_>) -> Result<PublishJob>;
    /// 领取最新目标的可构建任务：排队中，或租约已过期且未用尽重试。落后的未上线任务标记为
    /// superseded。没有可领取的任务时返回 None。
    async fn claim(
        &self,
        lease: std::time::Duration,
        max_attempts: i32,
    ) -> Result<Option<ClaimedJob>>;
    /// 续租；租约已失效时返回 false。
    async fn renew(&self, job: &ClaimedJob, lease: std::time::Duration) -> Result<bool>;
    /// 在同一 `REPEATABLE READ` 视图中确认任务仍为最新目标并导出公开数据；
    /// 已不是最新目标时返回 None。
    async fn export(&self, job: &ClaimedJob) -> Result<Option<ExportedSite>>;
    /// 锁定站点状态并原子切换线上版本，同时写入该 generation 的文章修订映射。
    async fn activate(
        &self,
        job: &ClaimedJob,
        manifest_key: &str,
        artifact_prefix: &str,
        posts: &[(Uuid, Uuid)],
    ) -> Result<Activation>;
    /// 标记已不是最新目标的任务为 superseded（仅当租约仍有效）。
    async fn supersede(&self, job: &ClaimedJob) -> Result<()>;
    /// 记录构建失败：未用尽重试且仍为最新目标时重新排队，并在 `retry_delay` 之后才允许再次
    /// 领取；否则标记为 failed（落后任务为 superseded）。租约已失效时不做修改。
    async fn fail(
        &self,
        job: &ClaimedJob,
        error: &str,
        max_attempts: i32,
        retry_delay: std::time::Duration,
    ) -> Result<()>;
    async fn active_site(&self) -> Result<Option<ActiveSite>>;
}

/// 构建得到的单个文件。
#[derive(Debug)]
pub struct BuiltFile {
    /// 相对产物根目录、以 `/` 分隔的路径。
    pub path: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// 静态站点构建器。
#[async_trait]
pub trait SiteBuilder: Send + Sync {
    /// 用快照构建站点并返回全部产物文件；失败时返回带原因的错误。
    async fn build(&self, snapshot: &[u8], sha256: &str, generation: i64)
    -> Result<Vec<BuiltFile>>;
}
