use crate::ports::{Credentials, IdentityRepository, NewUser, UserChanges};
use chrono::Utc;
use silva_domain::identity::{AuditLog, Principal, Role, User, validate_roles};
use silva_shared::{
    error::{AppError, Result},
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
    pub async fn login(&self, email: String, password: String) -> Result<LoginSession> {
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
            .map_err(|_| AppError::Validation("session lifetime too large".into()))?;
        self.repository
            .create_session(user_id, &hash, Utc::now() + lifetime)
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
    pub async fn logout(&self, principal: &Principal) -> Result<()> {
        self.repository.revoke_session(principal).await
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
    ) -> Result<User> {
        principal.require("users.manage")?;
        self.create(
            Some(principal.user.id),
            email,
            display_name,
            password,
            roles,
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
        self.create(None, email, display_name, password, vec!["admin".into()])
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
    ) -> Result<User> {
        let email = email.trim().to_lowercase();
        if email.len() > 254
            || !email.contains('@')
            || email.starts_with('@')
            || email.ends_with('@')
            || email.chars().any(char::is_whitespace)
        {
            return Err(AppError::Validation("invalid email".into()));
        }
        validate_name(&display_name)?;
        if password.len() < 12 || password.len() > 1024 {
            return Err(AppError::Validation(
                "password must contain 12..1024 bytes".into(),
            ));
        }
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
            )
            .await
    }

    #[tracing::instrument(name = "identity.update_user", skip_all)]
    pub async fn update_user(
        &self,
        principal: &Principal,
        id: Uuid,
        changes: UserChanges,
    ) -> Result<User> {
        principal.require("users.manage")?;
        if let Some(name) = &changes.display_name {
            validate_name(name)?;
        }
        if let Some(status) = &changes.status
            && !["active", "disabled"].contains(&status.as_str())
        {
            return Err(AppError::Validation(
                "status must be active or disabled".into(),
            ));
        }
        if let Some(roles) = &changes.roles {
            validate_roles(roles)?;
        }
        self.repository
            .update_user(principal.user.id, id, changes)
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

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 100 {
        return Err(AppError::Validation(
            "display_name must contain 1..100 bytes".into(),
        ));
    }
    Ok(())
}
