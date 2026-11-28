use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use silva_shared::error::{AppError, FieldError, FieldErrorCode, Result};
use uuid::Uuid;

pub const AUTHOR_PERMISSIONS: &[&str] = &[
    "posts.create",
    "posts.read_own",
    "posts.edit_own",
    "media.manage_own",
];
pub const EDITOR_PERMISSIONS: &[&str] = &[
    "posts.manage",
    "posts.publish",
    "media.manage",
    "taxonomy.manage",
    "series.manage",
    "profile.manage",
    "resources.manage",
    "ai.manage",
];
pub const ADMIN_PERMISSIONS: &[&str] = &[
    "users.manage",
    "roles.read",
    "ai.ingest",
    "audit.read",
    "system.read",
];
pub const ROLE_KEYS: &[&str] = &["author", "editor", "admin"];
/// 账户状态取值：active 为启用，disabled 为停用。
pub const USER_STATUSES: &[&str] = &["active", "disabled"];

pub fn permissions_for_role(role: &str) -> Vec<&'static str> {
    let mut permissions = AUTHOR_PERMISSIONS.to_vec();
    if role == "editor" || role == "admin" {
        permissions.extend_from_slice(EDITOR_PERMISSIONS);
    }
    if role == "admin" {
        permissions.extend_from_slice(ADMIN_PERMISSIONS);
    }
    permissions
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub status: String,
    pub roles: Vec<String>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
    /// 最近一次登录成功的时间；从未登录时为 None，序列化为 null。
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub last_login_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct Principal {
    pub session_id: Uuid,
    pub user: User,
    pub permissions: Vec<String>,
}

impl Principal {
    pub fn require(&self, permission: &str) -> Result<()> {
        if self.user.status == "active" && self.permissions.iter().any(|p| p == permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }

    /// 内容和媒体用例必须同时校验操作权限与资源归属。
    pub fn require_owned(&self, global: &str, owned: &str, owner: Option<Uuid>) -> Result<()> {
        if self.require(global).is_ok()
            || (owner == Some(self.user.id) && self.require(owned).is_ok())
        {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Role {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub permissions: Vec<String>,
    /// 分配了该角色的账户数，不区分启用或停用。
    pub user_count: i64,
}

/// 审计记录中的操作账户摘要，只含识别账户所需的非敏感字段。
#[derive(Debug, Serialize)]
pub struct AuditActor {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
}

#[derive(Debug, Serialize)]
pub struct AuditLog {
    pub id: Uuid,
    pub actor_user_id: Option<Uuid>,
    /// 操作账户摘要；系统或 CLI 操作为 None。
    pub actor: Option<AuditActor>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    /// 产生该记录的 HTTP 请求 ID，与响应头 `X-Request-Id` 一致；CLI 等非 HTTP 操作为 None。
    pub request_id: Option<Uuid>,
    /// 发起操作的客户端 IP；CLI 等非 HTTP 操作为 None。
    pub ip_address: Option<String>,
    /// 客户端报告的 User-Agent，超长时截断。
    pub user_agent: Option<String>,
    /// 非敏感的附加信息，例如变更字段名。
    pub metadata: serde_json::Value,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
}

/// 审计操作类型的筛选方式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditActionFilter {
    /// 与操作类型完全相同。
    Exact(String),
    /// 以该前缀开头；前缀保留末尾的 `.`，例如 `auth.*` 对应 `auth.`。
    Prefix(String),
}

/// 审计记录筛选条件，各条件以 AND 组合；时间范围含 `from`、不含 `to`。
#[derive(Clone, Debug, Default)]
pub struct AuditLogFilter {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub actor_user_id: Option<Uuid>,
    pub resource_id: Option<Uuid>,
    pub request_id: Option<Uuid>,
    pub action: Option<AuditActionFilter>,
    pub resource_type: Option<String>,
}

impl AuditLogFilter {
    /// 校验跨字段约束：同时给出时间上下界时，`from` 必须早于 `to`。
    pub fn validate(&self) -> Result<()> {
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from >= to
        {
            return Err(AppError::invalid(
                "to",
                FieldErrorCode::OutOfRange,
                "to must be later than from",
            ));
        }
        Ok(())
    }
}

/// 审计筛选中操作类型与资源类型的最大长度。
const AUDIT_FILTER_MAX_LEN: usize = 64;

/// 解析 `action` 筛选值：`[a-z0-9_.]` 组成的精确值，或以 `.*` 结尾的前缀匹配，总长 1..=64。
pub fn parse_audit_action(value: &str) -> std::result::Result<AuditActionFilter, FieldError> {
    let invalid = || {
        FieldError::new(
            "action",
            FieldErrorCode::InvalidValue,
            "action must be 1 to 64 characters of a-z, 0-9, _ and ., optionally ending with .*",
        )
    };
    if value.is_empty() || value.len() > AUDIT_FILTER_MAX_LEN {
        return Err(invalid());
    }
    let allowed = |text: &str| {
        text.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
    };
    match value.strip_suffix('*') {
        // 前缀匹配必须以 `.` 结束且前面至少有一个字符，避免 `*` 或 `.*` 匹配全部记录。
        Some(prefix) if prefix.len() > 1 && prefix.ends_with('.') && allowed(prefix) => {
            Ok(AuditActionFilter::Prefix(prefix.to_owned()))
        }
        Some(_) => Err(invalid()),
        None if allowed(value) => Ok(AuditActionFilter::Exact(value.to_owned())),
        None => Err(invalid()),
    }
}

/// 校验 `resource_type` 筛选值：`[a-z0-9_]`，长度 1..=64。
pub fn parse_audit_resource_type(value: &str) -> std::result::Result<String, FieldError> {
    if !value.is_empty()
        && value.len() <= AUDIT_FILTER_MAX_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        Ok(value.to_owned())
    } else {
        Err(FieldError::new(
            "resource_type",
            FieldErrorCode::InvalidValue,
            "resource_type must be 1 to 64 characters of a-z, 0-9 and _",
        ))
    }
}

pub fn validate_roles(roles: &[String]) -> Result<()> {
    if roles.is_empty() {
        return Err(AppError::invalid(
            "roles",
            FieldErrorCode::OutOfRange,
            "roles must contain at least one role",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut errors = Vec::new();
    for (index, role) in roles.iter().enumerate() {
        if !ROLE_KEYS.contains(&role.as_str()) {
            errors.push(FieldError::new(
                format!("roles[{index}]"),
                FieldErrorCode::InvalidValue,
                "role must be one of: author, editor, admin",
            ));
        } else if !seen.insert(role) {
            errors.push(FieldError::new(
                format!("roles[{index}]"),
                FieldErrorCode::Duplicate,
                "role is listed more than once",
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::Validation(errors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn role_hierarchy_does_not_grant_author_editor_actions() {
        assert!(!permissions_for_role("author").contains(&"posts.publish"));
        assert!(permissions_for_role("editor").contains(&"posts.publish"));
        assert!(!permissions_for_role("editor").contains(&"users.manage"));
        assert!(permissions_for_role("admin").contains(&"users.manage"));
    }

    #[test]
    fn audit_action_filter_accepts_exact_and_dot_star_prefix_only() {
        assert_eq!(
            parse_audit_action("auth.login"),
            Ok(AuditActionFilter::Exact("auth.login".into()))
        );
        assert_eq!(
            parse_audit_action("auth.*"),
            Ok(AuditActionFilter::Prefix("auth.".into()))
        );
        let too_long = "a".repeat(65);
        for invalid in [
            "",
            "*",
            ".*",
            "auth*",
            "a*b",
            "Auth.login",
            "auth-login",
            "auth.%",
            too_long.as_str(),
        ] {
            assert!(parse_audit_action(invalid).is_err(), "{invalid}");
        }
        assert!(parse_audit_action(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn audit_resource_type_and_time_range_are_validated() {
        assert_eq!(parse_audit_resource_type("user").as_deref(), Ok("user"));
        for invalid in ["", "User", "user.x", "a b"] {
            assert!(parse_audit_resource_type(invalid).is_err(), "{invalid}");
        }
        let now = Utc::now();
        let range = |from, to| AuditLogFilter {
            from: Some(from),
            to: Some(to),
            ..AuditLogFilter::default()
        };
        assert!(
            range(now, now + chrono::Duration::seconds(1))
                .validate()
                .is_ok()
        );
        assert!(range(now, now).validate().is_err());
        assert!(
            range(now + chrono::Duration::seconds(1), now)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn ownership_and_active_status_are_both_required() {
        let user_id = silva_shared::ids::new_id();
        let now = Utc::now();
        let mut principal = Principal {
            session_id: silva_shared::ids::new_id(),
            user: User {
                id: user_id,
                email: "author@example.test".into(),
                display_name: "Author".into(),
                status: "active".into(),
                roles: vec!["author".into()],
                created_at: now,
                updated_at: now,
                last_login_at: None,
            },
            permissions: AUTHOR_PERMISSIONS.iter().map(|key| (*key).into()).collect(),
        };
        assert!(
            principal
                .require_owned("posts.manage", "posts.edit_own", Some(user_id))
                .is_ok()
        );
        assert!(
            principal
                .require_owned(
                    "posts.manage",
                    "posts.edit_own",
                    Some(silva_shared::ids::new_id())
                )
                .is_err()
        );
        assert!(
            principal
                .require_owned("posts.manage", "posts.edit_own", None)
                .is_err()
        );
        principal.user.status = "disabled".into();
        assert!(
            principal
                .require_owned("posts.manage", "posts.edit_own", Some(user_id))
                .is_err()
        );
    }
}
