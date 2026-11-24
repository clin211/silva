use anyhow::{Context, bail};
use rand::RngCore;
use std::{env, fmt, net::SocketAddr, time::Duration};

/// JWT 签名密钥的最小字节数，与 HS256 的输出长度一致。
pub const JWT_SECRET_MIN_BYTES: usize = 32;

// DATABASE_URL 可能包含凭据，因此不实现 Debug，避免意外泄露。
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    /// `SILVA_ENV=production` 时为 true；生产环境要求显式配置 JWT 签名密钥。
    pub production: bool,
    /// `SILVA_JWT_SECRET` 原始值；未设置或为空时为 None。长度与生产要求由
    /// [`Config::resolve_jwt_secret`] 在 API 启动时校验，CLI 和 Worker 不依赖该变量。
    pub jwt_secret: Option<JwtSecret>,
    pub db_max_connections: u32,
    /// access token 有效期。
    pub access_token_ttl: Duration,
    /// refresh token 及其服务端会话的有效期；每次刷新从刷新时刻重新计算。
    pub refresh_token_ttl: Duration,
    pub worker_interval: Duration,
    pub api_rate_per_minute: u32,
    pub login_rate_per_minute: u32,
    /// 同一登录邮箱在失败窗口内连续失败多少次后锁定。
    pub login_lockout_threshold: u32,
    /// 账户登录锁定时长。
    pub login_lockout_duration: Duration,
}

/// HS256 签名密钥。Debug 输出不含密钥内容，避免写入日志。
#[derive(Clone, PartialEq, Eq)]
pub struct JwtSecret(Vec<u8>);

impl JwtSecret {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// 由操作系统随机源生成 32 字节密钥，仅用于开发、测试或不签发令牌的进程。
    pub fn generate() -> Self {
        let mut bytes = vec![0u8; JWT_SECRET_MIN_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for JwtSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JwtSecret(<redacted>)")
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        load_dotenv()?;
        let environment = env::var("SILVA_ENV").unwrap_or_else(|_| "development".into());
        if !["development", "test", "production"].contains(&environment.as_str()) {
            bail!("SILVA_ENV must be development, test or production");
        }
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        // 错误信息和日志不得包含数据库连接地址。
        let db =
            url::Url::parse(&database_url).map_err(|_| anyhow::anyhow!("invalid DATABASE_URL"))?;
        if !["postgres", "postgresql"].contains(&db.scheme()) {
            bail!("DATABASE_URL must use PostgreSQL");
        }
        let jwt_secret = match env::var("SILVA_JWT_SECRET") {
            Ok(value) if value.is_empty() => None,
            Ok(value) => Some(JwtSecret::new(value.into_bytes())),
            Err(env::VarError::NotPresent) => None,
            Err(env::VarError::NotUnicode(_)) => bail!("SILVA_JWT_SECRET must be valid UTF-8"),
        };
        Ok(Self {
            database_url,
            bind: env::var("SILVA_BIND")
                .unwrap_or_else(|_| "127.0.0.1:8088".into())
                .parse()
                .context("invalid SILVA_BIND")?,
            production: environment == "production",
            jwt_secret,
            db_max_connections: positive("SILVA_DB_MAX_CONNECTIONS", 10)?,
            access_token_ttl: seconds("SILVA_ACCESS_TOKEN_TTL_SECONDS", 900)?,
            refresh_token_ttl: seconds("SILVA_REFRESH_TOKEN_TTL_SECONDS", 604_800)?,
            worker_interval: seconds("SILVA_WORKER_INTERVAL_SECONDS", 60)?,
            api_rate_per_minute: positive("SILVA_API_RATE_PER_MINUTE", 600)?,
            login_rate_per_minute: positive("SILVA_LOGIN_RATE_PER_MINUTE", 10)?,
            login_lockout_threshold: positive("SILVA_LOGIN_LOCKOUT_THRESHOLD", 5)?,
            login_lockout_duration: seconds("SILVA_LOGIN_LOCKOUT_SECONDS", 900)?,
        })
    }

    /// 返回签发令牌的密钥：已配置时必须至少 32 字节；生产环境未配置时启动失败；
    /// 开发与测试环境未配置时随机生成并告警，进程重启后此前签发的令牌全部失效。
    pub fn resolve_jwt_secret(&self) -> anyhow::Result<JwtSecret> {
        match &self.jwt_secret {
            Some(secret) if secret.as_bytes().len() < JWT_SECRET_MIN_BYTES => {
                bail!("SILVA_JWT_SECRET must contain at least {JWT_SECRET_MIN_BYTES} bytes")
            }
            Some(secret) => Ok(secret.clone()),
            None if self.production => bail!("SILVA_JWT_SECRET is required in production"),
            None => {
                tracing::warn!(
                    "SILVA_JWT_SECRET is not set; using a random signing key, issued tokens become invalid after restart"
                );
                Ok(JwtSecret::generate())
            }
        }
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

fn seconds(name: &str, default: u32) -> anyhow::Result<Duration> {
    Ok(Duration::from_secs(u64::from(positive(name, default)?)))
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

    fn config(production: bool, jwt_secret: Option<&[u8]>) -> Config {
        Config {
            database_url: "postgres://unused".into(),
            bind: "127.0.0.1:0".parse().unwrap(),
            production,
            jwt_secret: jwt_secret.map(JwtSecret::new),
            db_max_connections: 1,
            access_token_ttl: Duration::from_secs(900),
            refresh_token_ttl: Duration::from_secs(604_800),
            worker_interval: Duration::from_secs(60),
            api_rate_per_minute: 1,
            login_rate_per_minute: 1,
            login_lockout_threshold: 1,
            login_lockout_duration: Duration::from_secs(1),
        }
    }

    #[test]
    fn jwt_secret_requires_minimum_length_and_is_mandatory_in_production() {
        let strong = [7u8; 32];
        assert_eq!(
            config(true, Some(&strong))
                .resolve_jwt_secret()
                .unwrap()
                .as_bytes(),
            strong
        );
        assert!(config(true, Some(&[7u8; 31])).resolve_jwt_secret().is_err());
        assert!(config(false, Some(b"short")).resolve_jwt_secret().is_err());
        assert!(config(true, None).resolve_jwt_secret().is_err());
        let generated = config(false, None).resolve_jwt_secret().unwrap();
        assert_eq!(generated.as_bytes().len(), JWT_SECRET_MIN_BYTES);
        assert_ne!(generated, config(false, None).resolve_jwt_secret().unwrap());
    }

    #[test]
    fn jwt_secret_debug_output_is_redacted() {
        let secret = JwtSecret::new(b"super-secret-signing-key-material".to_vec());
        assert!(!format!("{secret:?}").contains("super-secret"));
    }
}
