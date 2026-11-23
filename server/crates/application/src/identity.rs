use crate::ports::{
    Credentials, IdentityRepository, NewUser, RequestMeta, UserChanges, UserFilter,
};
use chrono::Utc;
use silva_domain::identity::{
    AuditLog, Principal, ROLE_KEYS, Role, USER_STATUSES, User, validate_roles,
};
use silva_shared::{
    error::{AppError, FieldError, FieldErrorCode, Result},
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

/// 账户列表关键字的最大字节数（去首尾空白后计算）。
pub const USER_QUERY_MAX_BYTES: usize = 100;

impl UserFilter {
    /// 规范化并校验列表筛选参数，一次返回全部字段错误。
    /// `q` 去首尾空白后为空视为未提供；`role`、`status` 必须是内置取值。
    pub fn new(q: Option<String>, role: Option<String>, status: Option<String>) -> Result<Self> {
        let mut errors = Vec::new();
        let q = q
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if q.as_ref()
            .is_some_and(|value| value.len() > USER_QUERY_MAX_BYTES)
        {
            errors.push(FieldError::new(
                "q",
                FieldErrorCode::OutOfRange,
                "q must contain at most 100 bytes",
            ));
        }
        if role
            .as_deref()
            .is_some_and(|value| !ROLE_KEYS.contains(&value))
        {
            errors.push(FieldError::new(
                "role",
                FieldErrorCode::InvalidValue,
                "role must be one of: author, editor, admin",
            ));
        }
        if status
            .as_deref()
            .is_some_and(|value| !USER_STATUSES.contains(&value))
        {
            errors.push(FieldError::new(
                "status",
                FieldErrorCode::InvalidValue,
                "status must be active or disabled",
            ));
        }
        if errors.is_empty() {
            Ok(Self { q, role, status })
        } else {
            Err(AppError::Validation(errors))
        }
    }
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
    pub async fn users(
        &self,
        principal: &Principal,
        page: &PageRequest,
        filter: &UserFilter,
    ) -> Result<Page<User>> {
        principal.require("users.manage")?;
        page.validate()?;
        self.repository.list_users(page, filter).await
    }

    #[tracing::instrument(name = "identity.user", skip_all)]
    pub async fn user(&self, principal: &Principal, id: Uuid) -> Result<User> {
        principal.require("users.manage")?;
        self.repository.find_user(id).await
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
            && !USER_STATUSES.contains(&status.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn field_codes(error: AppError) -> Vec<(String, FieldErrorCode)> {
        match error {
            AppError::Validation(errors) => errors
                .into_iter()
                .map(|error| (error.field.into_owned(), error.code))
                .collect(),
            other => panic!("期望校验错误，实际为 {other:?}"),
        }
    }

    #[test]
    fn user_filter_trims_query_and_treats_blank_as_absent() {
        let filter = UserFilter::new(Some("  Ada ".into()), None, None).unwrap();
        assert_eq!(filter.q.as_deref(), Some("Ada"));
        let blank = UserFilter::new(Some(" \t ".into()), None, None).unwrap();
        assert_eq!(blank, UserFilter::default());
    }

    #[test]
    fn user_filter_limits_query_bytes_after_trimming() {
        let padded = format!("  {}  ", "a".repeat(USER_QUERY_MAX_BYTES));
        assert!(UserFilter::new(Some(padded), None, None).is_ok());
        // 多字节字符按字节计数：34 个“长”为 102 字节。
        let error = UserFilter::new(Some("长".repeat(34)), None, None).unwrap_err();
        assert_eq!(
            field_codes(error),
            vec![("q".into(), FieldErrorCode::OutOfRange)]
        );
    }

    #[test]
    fn user_filter_accepts_builtin_values_and_reports_every_invalid_field() {
        let filter = UserFilter::new(None, Some("editor".into()), Some("disabled".into())).unwrap();
        assert_eq!(filter.role.as_deref(), Some("editor"));
        assert_eq!(filter.status.as_deref(), Some("disabled"));
        let error = UserFilter::new(Some("x".repeat(101)), Some("Admin".into()), Some("".into()))
            .unwrap_err();
        assert_eq!(
            field_codes(error),
            vec![
                ("q".into(), FieldErrorCode::OutOfRange),
                ("role".into(), FieldErrorCode::InvalidValue),
                ("status".into(), FieldErrorCode::InvalidValue),
            ]
        );
    }
}
