use crate::ports::{Credentials, IdentityRepository, NewUser, RequestMeta, UserChanges};
use chrono::Utc;
use silva_domain::identity::{AuditLog, Principal, Role, User, validate_roles};
use silva_shared::{
    error::{AppError, FieldErrorCode, Result},
    ids,
    pagination::{Page, PageRequest},
};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub struct IdentityService {
    repository: Arc<dyn IdentityRepository>,
    credentials: Arc<dyn Credentials>,
    session_ttl: Duration,
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
        }
    }

    #[tracing::instrument(name = "identity.login", skip_all)]
    pub async fn login(
        &self,
        email: String,
        password: String,
        meta: &RequestMeta,
    ) -> Result<LoginSession> {
        if password.len() > 1024 || email.len() > 254 {
            return Err(AppError::InvalidCredentials);
        }
        let record = self
            .repository
            .find_login(&email.trim().to_lowercase())
            .await?;
        let user_id = record.as_ref().map(|record| record.id);
        let valid = self
            .credentials
            .verify_password(password, record.map(|record| record.password_hash))
            .await?;
        let user_id = user_id
            .filter(|_| valid)
            .ok_or(AppError::InvalidCredentials)?;
        let token = self.credentials.new_session_token();
        let hash = self.credentials.token_hash(&token);
        let lifetime = chrono::Duration::from_std(self.session_ttl)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("session lifetime too large")))?;
        self.repository
            .create_session(user_id, &hash, Utc::now() + lifetime, meta)
            .await?;
        let principal = self.repository.principal(&hash).await?;
        Ok(LoginSession {
            csrf_token: self.credentials.csrf_token(&token),
            token,
            principal,
        })
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
