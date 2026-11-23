use async_trait::async_trait;
use chrono::{DateTime, Utc};
use silva_domain::identity::{AuditLog, Principal, Role, User};
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
}

pub struct LoginRecord {
    pub id: Uuid,
    pub password_hash: String,
}

pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub roles: Vec<String>,
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
    async fn find_login(&self, email: &str) -> Result<Option<LoginRecord>>;
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
    async fn list_users(&self, page: &PageRequest) -> Result<Page<User>>;
    async fn create_user(
        &self,
        actor: Option<Uuid>,
        user: NewUser,
        meta: &RequestMeta,
    ) -> Result<User>;
    /// 在同一事务中锁定账户、替换角色分配，并撤销已停用账户的会话。
    /// 修改后若不再有启用状态的 admin，返回 `Conflict("last_admin_required")` 并回滚。
    async fn update_user(
        &self,
        actor: Uuid,
        id: Uuid,
        changes: UserChanges,
        meta: &RequestMeta,
    ) -> Result<User>;
    async fn roles(&self) -> Result<Vec<Role>>;
    async fn audit_logs(&self, page: &PageRequest) -> Result<Page<AuditLog>>;
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
