use async_trait::async_trait;
use chrono::{DateTime, Utc};
use silva_domain::identity::{AuditLog, AuditLogFilter, Principal, Role, User};
use silva_shared::{
    error::Result,
    pagination::{Page, PageRequest},
};
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
    async fn create_session(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
        meta: &RequestMeta,
    ) -> Result<()>;
    /// 重新读取账户状态和角色权限，不缓存登录时的授权结果。
    async fn principal(&self, token_hash: &str) -> Result<Principal>;
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
    fn new_session_token(&self) -> String;
    fn token_hash(&self, token: &str) -> String;
    fn csrf_token(&self, token: &str) -> String;
}
