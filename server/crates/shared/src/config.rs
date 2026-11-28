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
    /// 仅在 API 只对可信内部服务开放、网关会覆盖 X-Real-IP 时开启，同时启用内部 SSR 接口。
    pub trust_proxy_ip_header: bool,
    pub api_rate_per_minute: u32,
    pub login_rate_per_minute: u32,
    /// 同一登录邮箱在失败窗口内连续失败多少次后锁定。
    pub login_lockout_threshold: u32,
    /// 账户登录锁定时长。
    pub login_lockout_duration: Duration,
    /// 私有对象存储；未配置 `SILVA_STORAGE_PROVIDER` 时为 None，媒体接口返回 503。
    pub storage: Option<StorageConfig>,
    /// 媒体查看链接的有效期，也用于待处理上传的清理期限。
    pub media_upload_ttl: Duration,
}

/// 对象存储服务商。两者都通过各自的原生签名协议访问私有 bucket。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageProvider {
    /// Cloudflare R2，使用 S3 兼容接口与 AWS SigV4 签名。
    R2,
    /// 阿里云 OSS，使用 OSS 原生接口与签名。
    AliyunOss,
}

/// 私有对象存储连接配置。Debug 输出不含访问密钥。
#[derive(Clone)]
pub struct StorageConfig {
    pub provider: StorageProvider,
    /// 服务端访问存储的 HTTPS 地址，同时用于生成浏览器直传地址。
    /// R2 形如 `https://<account_id>.r2.cloudflarestorage.com`，
    /// OSS 形如 `https://oss-cn-hangzhou.aliyuncs.com`（不含 bucket）。
    pub endpoint: String,
    pub bucket: String,
    /// 签名使用的区域：R2 默认 `auto`；OSS 必填，为不含 `oss-` 前缀的地域 ID，如 `cn-hangzhou`。
    pub region: String,
    /// bucket 内的根前缀，规范化为 `/` 或 `/a/b/`，便于多个环境共用一个 bucket。
    pub root: String,
    pub access_key_id: String,
    pub access_key_secret: StorageSecret,
}

/// 对象存储访问密钥。Debug 输出不含密钥内容，避免写入日志。
#[derive(Clone, PartialEq, Eq)]
pub struct StorageSecret(String);

impl StorageSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for StorageSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StorageSecret(<redacted>)")
    }
}

impl fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageConfig")
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("root", &self.root)
            .field("access_key_id", &self.access_key_id)
            .field("access_key_secret", &self.access_key_secret)
            .finish()
    }
}

impl StorageConfig {
    /// 从 `SILVA_STORAGE_*` 读取配置；`SILVA_STORAGE_PROVIDER` 未设置或为空时返回 None。
    /// 开发环境允许 `http://` 地址，生产环境只接受 HTTPS。
    pub fn from_env(production: bool) -> anyhow::Result<Option<Self>> {
        let provider = match optional("SILVA_STORAGE_PROVIDER")?.as_deref() {
            None => return Ok(None),
            Some("r2") => StorageProvider::R2,
            Some("oss") => StorageProvider::AliyunOss,
            Some(_) => bail!("SILVA_STORAGE_PROVIDER must be r2 or oss"),
        };
        let required = |name: &str| {
            optional(name)?
                .with_context(|| format!("{name} is required when SILVA_STORAGE_PROVIDER is set"))
        };
        let endpoint = required("SILVA_STORAGE_ENDPOINT")?;
        let parsed = url::Url::parse(&endpoint)
            .map_err(|_| anyhow::anyhow!("invalid SILVA_STORAGE_ENDPOINT"))?;
        let scheme_ok = parsed.scheme() == "https" || (!production && parsed.scheme() == "http");
        if !scheme_ok
            || parsed.host_str().is_none()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            bail!(
                "SILVA_STORAGE_ENDPOINT must be an origin without credentials, path, query or fragment"
            );
        }
        let root = normalize_root(optional("SILVA_STORAGE_ROOT")?.as_deref().unwrap_or("/"))?;
        let bucket = required("SILVA_STORAGE_BUCKET")?;
        if !bucket
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        {
            bail!("SILVA_STORAGE_BUCKET must contain only a-z, 0-9, - and .");
        }
        let region = match (provider, optional("SILVA_STORAGE_REGION")?) {
            (StorageProvider::R2, region) => region.unwrap_or_else(|| "auto".into()),
            (StorageProvider::AliyunOss, Some(region)) if region != "auto" => region,
            (StorageProvider::AliyunOss, _) => {
                bail!("SILVA_STORAGE_REGION is required for oss, e.g. cn-hangzhou")
            }
        };
        Ok(Some(Self {
            provider,
            endpoint: parsed.origin().ascii_serialization(),
            bucket,
            region,
            root,
            access_key_id: required("SILVA_STORAGE_ACCESS_KEY_ID")?,
            access_key_secret: StorageSecret::new(required("SILVA_STORAGE_ACCESS_KEY_SECRET")?),
        }))
    }
}

impl StorageConfig {
    /// 对象在 bucket 内的完整路径（不以 `/` 开头），与 OpenDAL 按 `root` 拼接的结果一致。
    pub fn object_path(&self, key: &str) -> String {
        format!("{}{key}", self.root.trim_start_matches('/'))
    }
}

/// 规范化根前缀为 `/` 或 `/a/b/`。只允许 `A-Za-z0-9._-` 组成的路径段，
/// 保证对象路径无需额外编码即可放入预签名地址。
fn normalize_root(value: &str) -> anyhow::Result<String> {
    let segments: Vec<&str> = value.split('/').filter(|s| !s.is_empty()).collect();
    let valid = value.starts_with('/')
        && segments.iter().all(|segment| {
            *segment != "."
                && *segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        });
    if !valid {
        bail!(
            "SILVA_STORAGE_ROOT must be an absolute prefix of A-Z, a-z, 0-9, ., _ and -, such as /silva/production/"
        );
    }
    Ok(if segments.is_empty() {
        "/".into()
    } else {
        format!("/{}/", segments.join("/"))
    })
}

/// 上传有效期上限：与 `uploads/` 前缀 1 天的生命周期规则一致，且远小于预签名地址的 7 天上限。
pub const MEDIA_UPLOAD_TTL_MAX_SECONDS: u32 = 86_400;

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
            trust_proxy_ip_header: match optional("SILVA_TRUST_PROXY_IP_HEADER")?.as_deref() {
                None | Some("false" | "0") => false,
                Some("true" | "1") => true,
                Some(_) => bail!("SILVA_TRUST_PROXY_IP_HEADER must be true or false"),
            },
            api_rate_per_minute: positive("SILVA_API_RATE_PER_MINUTE", 600)?,
            login_rate_per_minute: positive("SILVA_LOGIN_RATE_PER_MINUTE", 10)?,
            login_lockout_threshold: positive("SILVA_LOGIN_LOCKOUT_THRESHOLD", 5)?,
            login_lockout_duration: seconds("SILVA_LOGIN_LOCKOUT_SECONDS", 900)?,
            storage: StorageConfig::from_env(environment == "production")?,
            media_upload_ttl: upload_ttl()?,
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

/// 读取直传有效期，超过 [`MEDIA_UPLOAD_TTL_MAX_SECONDS`] 时启动失败。
fn upload_ttl() -> anyhow::Result<Duration> {
    let ttl = seconds("SILVA_MEDIA_UPLOAD_TTL_SECONDS", 900)?;
    if ttl.as_secs() > u64::from(MEDIA_UPLOAD_TTL_MAX_SECONDS) {
        bail!("SILVA_MEDIA_UPLOAD_TTL_SECONDS must be at most {MEDIA_UPLOAD_TTL_MAX_SECONDS}");
    }
    Ok(ttl)
}

/// 读取可选字符串配置；未设置或去空白后为空时返回 None。
fn optional(name: &str) -> anyhow::Result<Option<String>> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value.trim().to_owned())),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => bail!("{name} must be valid UTF-8"),
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
            trust_proxy_ip_header: false,
            api_rate_per_minute: 1,
            login_rate_per_minute: 1,
            login_lockout_threshold: 1,
            login_lockout_duration: Duration::from_secs(1),
            storage: None,
            media_upload_ttl: Duration::from_secs(900),
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
    fn storage_root_is_normalized_and_restricted() {
        assert_eq!(normalize_root("/").unwrap(), "/");
        assert_eq!(normalize_root("/silva/prod").unwrap(), "/silva/prod/");
        assert_eq!(normalize_root("//silva//dev/").unwrap(), "/silva/dev/");
        for invalid in ["silva/", "/a/../b", "/a/./b", "/a b/", "/a%2F/", "/长林/"] {
            assert!(normalize_root(invalid).is_err(), "{invalid}");
        }
        let mut storage = StorageConfig {
            provider: StorageProvider::R2,
            endpoint: "https://account.r2.cloudflarestorage.com".into(),
            bucket: "silva".into(),
            region: "auto".into(),
            root: "/".into(),
            access_key_id: "id".into(),
            access_key_secret: StorageSecret::new("secret"),
        };
        assert_eq!(storage.object_path("uploads/x"), "uploads/x");
        storage.root = "/silva/prod/".into();
        assert_eq!(storage.object_path("uploads/x"), "silva/prod/uploads/x");
    }

    #[test]
    fn storage_debug_output_is_redacted() {
        let storage = StorageConfig {
            provider: StorageProvider::R2,
            endpoint: "https://account.r2.cloudflarestorage.com".into(),
            bucket: "silva".into(),
            region: "auto".into(),
            root: "/".into(),
            access_key_id: "key-id".into(),
            access_key_secret: StorageSecret::new("very-secret-storage-key"),
        };
        let output = format!("{storage:?}");
        assert!(output.contains("key-id"));
        assert!(!output.contains("very-secret"));
    }

    #[test]
    fn jwt_secret_debug_output_is_redacted() {
        let secret = JwtSecret::new(b"super-secret-signing-key-material".to_vec());
        assert!(!format!("{secret:?}").contains("super-secret"));
    }
}
