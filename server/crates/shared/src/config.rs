use anyhow::{Context, bail};
use std::{env, net::SocketAddr, time::Duration};

// DATABASE_URL 可能包含凭据，因此不实现 Debug，避免意外泄露。
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    pub admin_origin: String,
    pub db_max_connections: u32,
    pub session_ttl: Duration,
    pub worker_interval: Duration,
    pub api_rate_per_minute: u32,
    pub login_rate_per_minute: u32,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        load_dotenv()?;
        let environment = env::var("SILVA_ENV").unwrap_or_else(|_| "development".into());
        if !["development", "test", "production"].contains(&environment.as_str()) {
            bail!("SILVA_ENV must be development, test or production");
        }
        let admin_origin =
            env::var("SILVA_ADMIN_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());
        validate_origin(&admin_origin, environment == "production")?;
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        // 错误信息和日志不得包含数据库连接地址。
        let db =
            url::Url::parse(&database_url).map_err(|_| anyhow::anyhow!("invalid DATABASE_URL"))?;
        if !["postgres", "postgresql"].contains(&db.scheme()) {
            bail!("DATABASE_URL must use PostgreSQL");
        }
        Ok(Self {
            database_url,
            bind: env::var("SILVA_BIND")
                .unwrap_or_else(|_| "127.0.0.1:8088".into())
                .parse()
                .context("invalid SILVA_BIND")?,
            admin_origin,
            db_max_connections: positive("SILVA_DB_MAX_CONNECTIONS", 10)?,
            session_ttl: Duration::from_secs(u64::from(positive(
                "SILVA_SESSION_TTL_SECONDS",
                43200,
            )?)),
            worker_interval: Duration::from_secs(u64::from(positive(
                "SILVA_WORKER_INTERVAL_SECONDS",
                60,
            )?)),
            api_rate_per_minute: positive("SILVA_API_RATE_PER_MINUTE", 600)?,
            login_rate_per_minute: positive("SILVA_LOGIN_RATE_PER_MINUTE", 10)?,
        })
    }
}

/// 在解析 CLI 参数前加载环境文件，此时尚不要求必填配置齐全。
pub fn load_dotenv() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("load .env"),
    }
}

pub fn validate_origin(origin: &str, production: bool) -> anyhow::Result<()> {
    let parsed = url::Url::parse(origin).context("invalid SILVA_ADMIN_ORIGIN")?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || parsed.origin().ascii_serialization() != origin
        || (production && parsed.scheme() != "https")
    {
        bail!(
            "SILVA_ADMIN_ORIGIN must be an exact origin (HTTPS in production), without path, credentials or trailing slash"
        );
    }
    Ok(())
}

fn positive(name: &str, default: u32) -> anyhow::Result<u32> {
    let value = match env::var(name) {
        Ok(v) => v.parse().with_context(|| format!("invalid {name}"))?,
        Err(env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    if value == 0 {
        bail!("{name} must be positive");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_origin_and_https_are_enforced() {
        assert!(validate_origin("https://admin.example.com", true).is_ok());
        assert!(validate_origin("http://localhost:5173", false).is_ok());
        for origin in [
            "http://admin.example.com",
            "https://admin.example.com/",
            "https://admin.example.com/path",
            "https://user:secret@admin.example.com",
            "https://admin.example.com?x=1",
        ] {
            assert!(validate_origin(origin, true).is_err());
        }
    }
}
