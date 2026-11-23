use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use silva_shared::error::{AppError, Result};
use uuid::Uuid;

pub const AUTHOR_PERMISSIONS: &[&str] = &[
    "posts.create",
    "posts.read_own",
    "posts.edit_own",
    "posts.submit_own",
    "media.manage_own",
    "jobs.read_own",
];
pub const EDITOR_PERMISSIONS: &[&str] = &[
    "posts.manage",
    "posts.review",
    "posts.publish",
    "media.manage",
    "taxonomy.manage",
    "resources.manage",
    "ai.manage",
    "jobs.read",
];
pub const ADMIN_PERMISSIONS: &[&str] = &[
    "users.manage",
    "roles.read",
    "ai.ingest",
    "publishing.retry",
    "audit.read",
    "system.read",
];
pub const ROLE_KEYS: &[&str] = &["author", "editor", "admin"];

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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
}

#[derive(Debug, Serialize)]
pub struct AuditLog {
    pub id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

pub fn validate_roles(roles: &[String]) -> Result<()> {
    if roles.is_empty()
        || roles.iter().any(|r| !ROLE_KEYS.contains(&r.as_str()))
        || roles.len()
            != roles
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
    {
        return Err(AppError::Validation(
            "roles must contain distinct built-in keys: author, editor, admin".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn role_hierarchy_does_not_grant_author_editor_actions() {
        assert!(!permissions_for_role("author").contains(&"posts.publish"));
        assert!(permissions_for_role("editor").contains(&"posts.review"));
        assert!(!permissions_for_role("editor").contains(&"users.manage"));
        assert!(permissions_for_role("admin").contains(&"users.manage"));
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
