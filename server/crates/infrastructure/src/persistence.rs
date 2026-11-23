use async_trait::async_trait;
use chrono::{DateTime, Utc};
use silva_application::ports::{IdentityRepository, LoginRecord, NewUser, UserChanges};
use silva_domain::identity::{AuditLog, Principal, ROLE_KEYS, Role, User, permissions_for_role};
use silva_shared::metrics::METRICS;
use silva_shared::{
    config::Config,
    error::{AppError, Result},
    ids,
    pagination::{Page, PageRequest},
};
use sqlx::{
    ConnectOptions, Postgres, Transaction, pool::PoolConnection, postgres::PgConnectOptions,
};
use sqlx::{
    PgConnection, PgPool, Row,
    postgres::{PgPoolOptions, PgRow},
};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub async fn connect(config: &Config) -> anyhow::Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET timezone = 'UTC'")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("SET statement_timeout = '10s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        // 禁止 SQLx 输出 SQL 与绑定值；采用只含固定操作名的数据库 span。
        .connect_with(PgConnectOptions::from_str(&config.database_url)?.disable_statement_logging())
        .await?)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    MIGRATOR.run(pool).await?;
    seed_builtin_roles(pool).await?;
    Ok(())
}

pub async fn check_schema(pool: &PgPool) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    sqlx::query("SELECT set_config('silva.test_id', $1, true)")
        .bind(ids::new_id().to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT set_config('silva.test_other_id', $1, true)")
        .bind(ids::new_id().to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::raw_sql(include_str!("../../../tests/sql/schema_invariants.sql"))
        .execute(&mut *tx)
        .await?;
    tx.rollback().await?;
    Ok(())
}

#[tracing::instrument(name = "db.ready", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
pub async fn ready(pool: &PgPool) -> anyhow::Result<()> {
    let _timer = METRICS.db_operation("ready");
    let mut connection = acquire(pool).await?;
    sqlx::query("SELECT singleton FROM site_state WHERE singleton")
        .fetch_one(&mut *connection)
        .await
        .map_err(query_error)?;
    // 就绪检查会校验数据库迁移版本和校验和，但不执行迁移。
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut *connection)
            .await
            .map_err(query_error)?;
    let expected: Vec<_> = MIGRATOR.iter().collect();
    anyhow::ensure!(
        applied.len() == expected.len()
            && applied.iter().zip(expected).all(
                |((version, checksum, success), migration)| *success
                    && *version == migration.version
                    && checksum.as_slice() == migration.checksum.as_ref()
            ),
        "database migrations do not match binary"
    );
    Ok(())
}

#[tracing::instrument(name = "db.maintain_sessions", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
pub async fn maintain_sessions(pool: &PgPool) -> anyhow::Result<u64> {
    let _timer = METRICS.db_operation("maintain_sessions");
    Ok(sqlx::query("DELETE FROM sessions s USING users u WHERE s.user_id = u.id AND (s.expires_at <= now() OR u.status <> 'active')").execute(&mut *acquire(pool).await?).await.map_err(query_error)?.rows_affected())
}

async fn seed_builtin_roles(pool: &PgPool) -> anyhow::Result<()> {
    let mut tx = begin(pool).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(782039415)")
        .execute(&mut *tx)
        .await?;
    for key in ROLE_KEYS {
        let role_id: Uuid = sqlx::query_scalar("INSERT INTO roles (id, key, name) VALUES ($1, $2, $2) ON CONFLICT (key) DO UPDATE SET name = EXCLUDED.name RETURNING id")
            .bind(ids::new_id()).bind(key).fetch_one(&mut *tx).await?;
        for permission in permissions_for_role(key) {
            let permission_id: Uuid = sqlx::query_scalar("INSERT INTO permissions (id, key, description) VALUES ($1,$2,$2) ON CONFLICT (key) DO UPDATE SET description = EXCLUDED.description RETURNING id")
                .bind(ids::new_id()).bind(permission).fetch_one(&mut *tx).await?;
            sqlx::query("INSERT INTO role_permissions (role_id, permission_id) VALUES ($1,$2) ON CONFLICT DO NOTHING").bind(role_id).bind(permission_id).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Clone)]
pub struct PgIdentityRepository {
    pool: PgPool,
}
impl PgIdentityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const USER_SELECT: &str = "SELECT u.id,u.email,u.display_name,u.status,u.created_at,u.updated_at, ARRAY(SELECT r.key FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=u.id ORDER BY r.key) AS roles FROM users u";

fn db(error: sqlx::Error) -> AppError {
    if !matches!(error, sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed) {
        record_db_error(&error);
    }
    if let sqlx::Error::Database(database) = &error
        && database.is_unique_violation()
    {
        return AppError::Conflict("email_in_use");
    }
    AppError::Internal(error.into())
}

fn read_user(row: &PgRow) -> std::result::Result<User, sqlx::Error> {
    Ok(User {
        id: row.try_get("id")?,
        email: row.try_get("email")?,
        display_name: row.try_get("display_name")?,
        status: row.try_get("status")?,
        roles: row.try_get("roles")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn user(connection: &mut PgConnection, id: Uuid) -> Result<User> {
    let row = sqlx::query(&format!("{USER_SELECT} WHERE u.id=$1"))
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(db)?
        .ok_or(AppError::NotFound)?;
    read_user(&row).map_err(db)
}

async fn audit(
    connection: &mut PgConnection,
    actor: Option<Uuid>,
    action: &str,
    resource: Uuid,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_logs (id, actor_user_id, action, resource_type, resource_id) VALUES ($1,$2,$3,'user',$4)")
        .bind(ids::new_id()).bind(actor).bind(action).bind(resource).execute(connection).await.map_err(db)?;
    Ok(())
}

async fn assign_roles(connection: &mut PgConnection, id: Uuid, roles: &[String]) -> Result<()> {
    sqlx::query("DELETE FROM user_roles WHERE user_id=$1")
        .bind(id)
        .execute(&mut *connection)
        .await
        .map_err(db)?;
    let result = sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) SELECT $1, id FROM roles WHERE key = ANY($2)",
    )
    .bind(id)
    .bind(roles)
    .execute(connection)
    .await
    .map_err(db)?;
    if result.rows_affected() != roles.len() as u64 {
        return Err(AppError::Validation(
            "unknown role; run migrations before seeding".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl IdentityRepository for PgIdentityRepository {
    #[tracing::instrument(name = "db.find_login", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn find_login(&self, email: &str) -> Result<Option<LoginRecord>> {
        let _timer = METRICS.db_operation("find_login");
        sqlx::query("SELECT id,password_hash FROM users WHERE lower(email)=$1 AND status='active'")
            .bind(email)
            .fetch_optional(&mut *acquire(&self.pool).await.map_err(db)?)
            .await
            .map_err(db)?
            .map(|row| {
                Ok(LoginRecord {
                    id: row.try_get("id").map_err(db)?,
                    password_hash: row.try_get("password_hash").map_err(db)?,
                })
            })
            .transpose()
    }

    #[tracing::instrument(name = "db.create_session", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn create_session(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let _timer = METRICS.db_operation("create_session");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let active: Option<String> =
            sqlx::query_scalar("SELECT status FROM users WHERE id=$1 FOR UPDATE")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        if active.as_deref() != Some("active") {
            return Err(AppError::InvalidCredentials);
        }
        sqlx::query("INSERT INTO sessions (id,user_id,token_hash,expires_at,last_seen_at) VALUES ($1,$2,$3,$4,now())")
            .bind(ids::new_id()).bind(user_id).bind(token_hash).bind(expires_at).execute(&mut *tx).await.map_err(db)?;
        sqlx::query("UPDATE users SET last_login_at=now() WHERE id=$1")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit(&mut tx, Some(user_id), "auth.login", user_id).await?;
        tx.commit().await.map_err(db)?;
        Ok(())
    }

    #[tracing::instrument(name = "db.principal", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn principal(&self, token_hash: &str) -> Result<Principal> {
        let _timer = METRICS.db_operation("principal");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let row = sqlx::query("SELECT s.id,s.user_id FROM sessions s JOIN users u ON u.id=s.user_id WHERE token_hash=$1 AND s.expires_at>now() AND u.status='active'")
            .bind(token_hash).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(AppError::Unauthorized)?;
        let session_id = row.try_get("id").map_err(db)?;
        let user_id = row.try_get("user_id").map_err(db)?;
        let user = user(&mut tx, user_id).await?;
        if user.status != "active" {
            return Err(AppError::Unauthorized);
        }
        let permissions = sqlx::query_scalar("SELECT DISTINCT p.key FROM permissions p JOIN role_permissions rp ON rp.permission_id=p.id JOIN user_roles ur ON ur.role_id=rp.role_id WHERE ur.user_id=$1 ORDER BY p.key")
            .bind(user_id).fetch_all(&mut *tx).await.map_err(db)?;
        sqlx::query("UPDATE sessions SET last_seen_at=now() WHERE id=$1 AND (last_seen_at IS NULL OR last_seen_at<now()-interval '5 minutes')").bind(session_id).execute(&mut *tx).await.map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Principal {
            session_id,
            user,
            permissions,
        })
    }

    #[tracing::instrument(name = "db.revoke_session", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn revoke_session(&self, principal: &Principal) -> Result<()> {
        let _timer = METRICS.db_operation("revoke_session");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("DELETE FROM sessions WHERE id=$1")
            .bind(principal.session_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        audit(
            &mut tx,
            Some(principal.user.id),
            "auth.logout",
            principal.user.id,
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(())
    }

    #[tracing::instrument(name = "db.list_users", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn list_users(&self, page: &PageRequest) -> Result<Page<User>> {
        let _timer = METRICS.db_operation("list_users");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let total = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let rows = sqlx::query(&format!(
            "{USER_SELECT} ORDER BY u.created_at DESC,u.id DESC LIMIT $1 OFFSET $2"
        ))
        .bind(i64::from(page.per_page))
        .bind(page.offset())
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        let data = rows
            .iter()
            .map(read_user)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Page {
            data,
            pagination: page.metadata(total),
        })
    }

    #[tracing::instrument(name = "db.create_user", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn create_user(&self, actor: Option<Uuid>, new: NewUser) -> Result<User> {
        let _timer = METRICS.db_operation("create_user");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("INSERT INTO users (id,email,password_hash,display_name) VALUES ($1,$2,$3,$4)")
            .bind(new.id)
            .bind(new.email)
            .bind(new.password_hash)
            .bind(new.display_name)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        assign_roles(&mut tx, new.id, &new.roles).await?;
        audit(
            &mut tx,
            actor,
            if actor.is_some() {
                "users.create"
            } else {
                "users.bootstrap"
            },
            new.id,
        )
        .await?;
        let result = user(&mut tx, new.id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }

    #[tracing::instrument(name = "db.update_user", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn update_user(&self, actor: Uuid, id: Uuid, changes: UserChanges) -> Result<User> {
        let _timer = METRICS.db_operation("update_user");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM users WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        if exists.is_none() {
            return Err(AppError::NotFound);
        }
        sqlx::query("UPDATE users SET display_name=COALESCE($2,display_name),status=COALESCE($3,status) WHERE id=$1")
            .bind(id).bind(changes.display_name.map(|s| s.trim().to_owned())).bind(&changes.status).execute(&mut *tx).await.map_err(db)?;
        if let Some(roles) = changes.roles {
            assign_roles(&mut tx, id, &roles).await?;
        }
        if changes.status.as_deref() == Some("disabled") {
            sqlx::query("DELETE FROM sessions WHERE user_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        audit(&mut tx, Some(actor), "users.update", id).await?;
        let result = user(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }

    #[tracing::instrument(name = "db.roles", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn roles(&self) -> Result<Vec<Role>> {
        let _timer = METRICS.db_operation("roles");
        let rows = sqlx::query("SELECT r.id,r.key,r.name,ARRAY(SELECT p.key FROM permissions p JOIN role_permissions rp ON rp.permission_id=p.id WHERE rp.role_id=r.id ORDER BY p.key) AS permissions FROM roles r ORDER BY r.key").fetch_all(&mut *acquire(&self.pool).await.map_err(db)?).await.map_err(db)?;
        rows.into_iter()
            .map(|row| {
                Ok(Role {
                    id: row.try_get("id").map_err(db)?,
                    key: row.try_get("key").map_err(db)?,
                    name: row.try_get("name").map_err(db)?,
                    permissions: row.try_get("permissions").map_err(db)?,
                })
            })
            .collect()
    }

    #[tracing::instrument(name = "db.audit_logs", skip_all, fields(otel.kind = "client", db.system.name = "postgresql", otel.status_code = tracing::field::Empty))]
    async fn audit_logs(&self, page: &PageRequest) -> Result<Page<AuditLog>> {
        let _timer = METRICS.db_operation("audit_logs");
        let mut tx = begin(&self.pool).await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let total = sqlx::query_scalar("SELECT count(*) FROM audit_logs")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        let rows = sqlx::query("SELECT id,actor_user_id,action,resource_type,resource_id,created_at FROM audit_logs ORDER BY created_at DESC,id DESC LIMIT $1 OFFSET $2")
            .bind(i64::from(page.per_page)).bind(page.offset()).fetch_all(&mut *tx).await.map_err(db)?;
        let data = rows
            .into_iter()
            .map(|row| {
                Ok(AuditLog {
                    id: row.try_get("id")?,
                    actor_user_id: row.try_get("actor_user_id")?,
                    action: row.try_get("action")?,
                    resource_type: row.try_get("resource_type")?,
                    resource_id: row.try_get("resource_id")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(Page {
            data,
            pagination: page.metadata(total),
        })
    }
}

pub fn update_pool_metrics(pool: &PgPool) {
    METRICS.db_connections.set(i64::from(pool.size()));
    METRICS.db_idle.set(pool.num_idle() as i64);
    METRICS
        .db_max
        .set(i64::from(pool.options().get_max_connections()));
}

fn query_error(error: sqlx::Error) -> sqlx::Error {
    record_db_error(&error);
    error
}

fn record_db_error(error: &sqlx::Error) {
    tracing::Span::current().record("otel.status_code", "ERROR");
    let kind = match error {
        sqlx::Error::PoolTimedOut => "acquire_timeout",
        sqlx::Error::PoolClosed => "pool_closed",
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) => "connection",
        sqlx::Error::Database(_) => "database",
        _ => "other",
    };
    METRICS.db_errors.with_label_values(&[kind]).inc();
}

async fn acquire(pool: &PgPool) -> std::result::Result<PoolConnection<Postgres>, sqlx::Error> {
    let started = Instant::now();
    let result = pool.acquire().await;
    METRICS
        .db_acquire
        .with_label_values(&["connection"])
        .observe(started.elapsed().as_secs_f64());
    if let Err(error @ (sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed)) = &result {
        record_db_error(error);
    }
    result
}

async fn begin(pool: &PgPool) -> std::result::Result<Transaction<'static, Postgres>, sqlx::Error> {
    let started = Instant::now();
    let result = pool.begin().await;
    METRICS
        .db_acquire
        .with_label_values(&["transaction"])
        .observe(started.elapsed().as_secs_f64());
    if let Err(error @ (sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed)) = &result {
        record_db_error(error);
    }
    result
}
