use crate::{
    login_lockout::{Admission, LoginLockout},
    ports::{Credentials, IdentityRepository, LoginFailure, NewUser, RequestMeta, UserChanges},
};
use chrono::Utc;
use silva_domain::identity::{AuditLog, Principal, Role, User, validate_roles};
use silva_shared::{
    error::{AppError, FieldErrorCode, Result},
    ids,
    pagination::{Page, PageRequest},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub struct IdentityService {
    repository: Arc<dyn IdentityRepository>,
    credentials: Arc<dyn Credentials>,
    session_ttl: Duration,
    lockout: LoginLockout,
}

/// 一次已通过锁定检查的登录尝试结果。
enum LoginAttempt {
    Success(Box<LoginSession>),
    Failed {
        user_id: Option<Uuid>,
        reason: LoginFailure,
    },
}

/// 管理员修改账户的输入；密码为明文，由用例哈希后再交给仓储。
#[derive(Default)]
pub struct UserUpdate {
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub roles: Option<Vec<String>>,
    pub password: Option<String>,
}

pub struct LoginSession {
    pub token: String,
    pub csrf_token: String,
    pub principal: Principal,
}

impl IdentityService {
    pub fn new(
        repository: Arc<dyn IdentityRepository>,
        credentials: Arc<dyn Credentials>,
        session_ttl: Duration,
    ) -> Self {
        Self {
            repository,
            credentials,
            session_ttl,
            lockout: LoginLockout::default(),
        }
    }

    /// 替换登录锁定策略；默认为 5 次失败锁定 900 秒。
    pub fn with_login_lockout(mut self, lockout: LoginLockout) -> Self {
        self.lockout = lockout;
        self
    }

    /// 登录：按规范化邮箱执行连续失败锁定，凭据失败与锁定拒绝均写入 `auth.login_failed` 审计。
    ///
    /// 审计写入失败与其他管理操作一致按内部错误返回（500），不会因此放行登录；
    /// 已发生的凭据失败仍然计入锁定。
    #[tracing::instrument(name = "identity.login", skip_all)]
    pub async fn login(
        &self,
        email: String,
        password: String,
        meta: &RequestMeta,
    ) -> Result<LoginSession> {
        let key = email.trim().to_lowercase();
        if key.len() > 254 {
            // 超长邮箱不可能对应账户，不进入锁定表，避免超长键占用内存。
            self.repository
                .record_login_failure(None, LoginFailure::InvalidCredentials, meta)
                .await?;
            return Err(AppError::InvalidCredentials);
        }
        if let Admission::Locked {
            retry_after,
            first_rejection,
        } = self.lockout.admit(&key, Instant::now())
        {
            // 锁定期内不校验密码；同一轮锁定只在首次被拒时写审计，防止审计表被刷爆。
            if first_rejection {
                let user_id = self.repository.find_login(&key).await?.map(|r| r.id);
                self.repository
                    .record_login_failure(user_id, LoginFailure::Locked, meta)
                    .await?;
            }
            return Err(AppError::LoginLocked { retry_after });
        }
        let attempt = match self.attempt_login(&key, password, meta).await {
            Ok(attempt) => attempt,
            Err(error) => {
                // 基础设施故障不属于凭据失败，撤回预先计入的次数。
                self.lockout.abandon(&key);
                return Err(error);
            }
        };
        match attempt {
            LoginAttempt::Success(session) => {
                self.lockout.succeed(&key);
                Ok(*session)
            }
            LoginAttempt::Failed { user_id, reason } => {
                self.repository
                    .record_login_failure(user_id, reason, meta)
                    .await?;
                Err(AppError::InvalidCredentials)
            }
        }
    }

    /// 校验凭据并创建会话。账户不存在、停用或密码错误时都执行一次 Argon2 校验，
    /// 对外统一返回 `invalid_credentials`，只在审计中区分原因。
    async fn attempt_login(
        &self,
        email: &str,
        password: String,
        meta: &RequestMeta,
    ) -> Result<LoginAttempt> {
        let record = self.repository.find_login(email).await?;
        let (account, password_hash) = record
            .map(|record| ((record.id, record.active), record.password_hash))
            .unzip();
        let valid = password.len() <= 1024
            && self
                .credentials
                .verify_password(password, password_hash)
                .await?;
        let user_id = match account {
            None => {
                return Ok(LoginAttempt::Failed {
                    user_id: None,
                    reason: LoginFailure::InvalidCredentials,
                });
            }
            Some((id, _)) if !valid => {
                return Ok(LoginAttempt::Failed {
                    user_id: Some(id),
                    reason: LoginFailure::InvalidCredentials,
                });
            }
            Some((id, false)) => {
                return Ok(LoginAttempt::Failed {
                    user_id: Some(id),
                    reason: LoginFailure::AccountDisabled,
                });
            }
            Some((id, true)) => id,
        };
        let token = self.credentials.new_session_token();
        let hash = self.credentials.token_hash(&token);
        let lifetime = chrono::Duration::from_std(self.session_ttl)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("session lifetime too large")))?;
        match self
            .repository
            .create_session(user_id, &hash, Utc::now() + lifetime, meta)
            .await
        {
            Ok(()) => {}
            // 校验密码后账户被并发停用。
            Err(AppError::InvalidCredentials) => {
                return Ok(LoginAttempt::Failed {
                    user_id: Some(user_id),
                    reason: LoginFailure::AccountDisabled,
                });
            }
            Err(error) => return Err(error),
        }
        let principal = self.repository.principal(&hash).await?;
        Ok(LoginAttempt::Success(Box::new(LoginSession {
            csrf_token: self.credentials.csrf_token(&token),
            token,
            principal,
        })))
    }

    #[tracing::instrument(name = "identity.authenticate", skip_all)]
    pub async fn authenticate(&self, token: &str) -> Result<Principal> {
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::Unauthorized);
        }
        self.repository
            .principal(&self.credentials.token_hash(token))
            .await
    }

    pub fn csrf_token(&self, token: &str) -> String {
        self.credentials.csrf_token(token)
    }

    #[tracing::instrument(name = "identity.logout", skip_all)]
    pub async fn logout(&self, principal: &Principal, meta: &RequestMeta) -> Result<()> {
        self.repository.revoke_session(principal, meta).await
    }

    /// 当前账户修改自己的密码：校验当前密码，保留本会话并撤销其他会话。
    #[tracing::instrument(name = "identity.change_password", skip_all)]
    pub async fn change_password(
        &self,
        principal: &Principal,
        current_password: String,
        new_password: String,
        meta: &RequestMeta,
    ) -> Result<()> {
        validate_password("new_password", &new_password)?;
        if current_password.len() > 1024 {
            return Err(current_password_invalid());
        }
        if current_password == new_password {
            return Err(AppError::invalid(
                "new_password",
                FieldErrorCode::Unchanged,
                "new password must differ from current password",
            ));
        }
        let stored = self.repository.password_hash(principal.user.id).await?;
        // 账户哈希缺失时 verify_password 仍执行虚拟校验并返回 false。
        if !self
            .credentials
            .verify_password(current_password, stored)
            .await?
        {
            return Err(current_password_invalid());
        }
        let password_hash = self.credentials.hash_password(new_password).await?;
        self.repository
            .change_password(principal, password_hash, meta)
            .await
    }

    #[tracing::instrument(name = "identity.users", skip_all)]
    pub async fn users(&self, principal: &Principal, page: &PageRequest) -> Result<Page<User>> {
        principal.require("users.manage")?;
        page.validate()?;
        self.repository.list_users(page).await
    }

    #[tracing::instrument(name = "identity.create_user", skip_all)]
    pub async fn create_user(
        &self,
        principal: &Principal,
        email: String,
        display_name: String,
        password: String,
        roles: Vec<String>,
        meta: &RequestMeta,
    ) -> Result<User> {
        principal.require("users.manage")?;
        self.create(
            Some(principal.user.id),
            email,
            display_name,
            password,
            roles,
            meta,
        )
        .await
    }

    /// 仅供 CLI 初始化管理员，不提供公开注册入口。
    #[tracing::instrument(name = "identity.seed_admin", skip_all)]
    pub async fn seed_admin(
        &self,
        email: String,
        display_name: String,
        password: String,
    ) -> Result<User> {
        self.create(
            None,
            email,
            display_name,
            password,
            vec!["admin".into()],
            &RequestMeta::default(),
        )
        .await
    }

    #[tracing::instrument(name = "identity.create", skip_all)]
    async fn create(
        &self,
        actor: Option<Uuid>,
        email: String,
        display_name: String,
        password: String,
        roles: Vec<String>,
        meta: &RequestMeta,
    ) -> Result<User> {
        let email = email.trim().to_lowercase();
        if email.len() > 254
            || !email.contains('@')
            || email.starts_with('@')
            || email.ends_with('@')
            || email.chars().any(char::is_whitespace)
        {
            return Err(AppError::invalid(
                "email",
                FieldErrorCode::InvalidFormat,
                "email must be a valid address of at most 254 bytes",
            ));
        }
        validate_name(&display_name)?;
        validate_password("password", &password)?;
        validate_roles(&roles)?;
        let password_hash = self.credentials.hash_password(password).await?;
        self.repository
            .create_user(
                actor,
                NewUser {
                    id: ids::new_id(),
                    email,
                    display_name: display_name.trim().into(),
                    password_hash,
                    roles,
                },
                meta,
            )
            .await
    }

    #[tracing::instrument(name = "identity.update_user", skip_all)]
    pub async fn update_user(
        &self,
        principal: &Principal,
        id: Uuid,
        update: UserUpdate,
        meta: &RequestMeta,
    ) -> Result<User> {
        principal.require("users.manage")?;
        let changes = UserChanges {
            display_name: update.display_name,
            status: update.status,
            roles: update.roles,
            password_hash: None,
        };
        if let Some(name) = &changes.display_name {
            validate_name(name)?;
        }
        if let Some(status) = &changes.status
            && !["active", "disabled"].contains(&status.as_str())
        {
            return Err(AppError::invalid(
                "status",
                FieldErrorCode::InvalidValue,
                "status must be active or disabled",
            ));
        }
        if let Some(roles) = &changes.roles {
            validate_roles(roles)?;
        }
        if let Some(password) = &update.password {
            validate_password("password", password)?;
        }
        let changes = match update.password {
            Some(password) => UserChanges {
                password_hash: Some(self.credentials.hash_password(password).await?),
                ..changes
            },
            None => changes,
        };
        self.repository
            .update_user(principal.user.id, id, changes, meta)
            .await
    }

    #[tracing::instrument(name = "identity.roles", skip_all)]
    pub async fn roles(&self, principal: &Principal) -> Result<Vec<Role>> {
        principal.require("roles.read")?;
        self.repository.roles().await
    }

    #[tracing::instrument(name = "identity.audit_logs", skip_all)]
    pub async fn audit_logs(
        &self,
        principal: &Principal,
        page: &PageRequest,
    ) -> Result<Page<AuditLog>> {
        principal.require("audit.read")?;
        page.validate()?;
        self.repository.audit_logs(page).await
    }
}

fn validate_password(field: &'static str, password: &str) -> Result<()> {
    if password.len() < 12 || password.len() > 1024 {
        return Err(AppError::invalid(
            field,
            FieldErrorCode::OutOfRange,
            "password must contain 12 to 1024 bytes",
        ));
    }
    Ok(())
}

fn current_password_invalid() -> AppError {
    AppError::invalid(
        "current_password",
        FieldErrorCode::Incorrect,
        "current password is incorrect",
    )
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 100 {
        return Err(AppError::invalid(
            "display_name",
            FieldErrorCode::OutOfRange,
            "display_name must contain 1 to 100 bytes",
        ));
    }
    Ok(())
}
