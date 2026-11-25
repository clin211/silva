//! 基于 Apache OpenDAL 的私有对象存储适配器，支持 Cloudflare R2 与阿里云 OSS。
//!
//! - R2 走 S3 兼容接口与 AWS SigV4，条件写入使用 `If-None-Match: *`。
//! - OSS 走原生接口与签名，条件写入使用 `x-oss-forbid-overwrite: true`；bucket 开启版本控制时
//!   OSS 会忽略该请求头，因此媒体 bucket 不应开启版本控制。
//!
//! 使用 reqsign 为服务端的流式 PUT 和短期 GET 签名。PUT 设置实际类型和声明长度，
//! 使用有背压的请求体将输入分块转发给对象存储。
//!
//! 只使用配置中的显式密钥，不读取运行环境的默认凭据链或云主机元数据。

use async_trait::async_trait;
use opendal::{
    ErrorKind, Operator,
    layers::{RetryLayer, TimeoutLayer},
    services,
};
use silva_application::ports::{ObjectStorage, PutOutcome, StoredObject, StreamedObject};
use silva_shared::{
    config::{StorageConfig, StorageProvider},
    error::{AppError, Result},
};
use std::time::Duration;
use url::Url;

/// 单次存储操作的超时时间；10 MiB 以内的对象足够完成读写。
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
/// 流式 PUT 包含客户端传输与图片解码时间。
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);
pub struct OpendalStorage {
    operator: Operator,
    client: reqwest::Client,
    signer: ObjectSigner,
    config: StorageConfig,
}

/// 对象请求签名器；`base` 为对象路径之前的地址部分。
enum ObjectSigner {
    R2 {
        signer: reqsign_core::Signer<reqsign_aws_v4::Credential>,
        base: String,
    },
    Oss {
        signer: reqsign_core::Signer<reqsign_aliyun_oss::Credential>,
        base: String,
    },
}

impl ObjectSigner {
    fn new(config: &StorageConfig) -> anyhow::Result<Self> {
        let secret = config.access_key_secret.expose();
        Ok(match config.provider {
            // 与 OpenDAL 的 S3 服务一致使用路径风格：`<endpoint>/<bucket>/<object>`。
            StorageProvider::R2 => Self::R2 {
                signer: reqsign_core::Signer::new(
                    reqsign_core::Context::new(),
                    reqsign_aws_v4::StaticCredentialProvider::new(&config.access_key_id, secret),
                    reqsign_aws_v4::RequestSigner::new("s3", &config.region),
                ),
                base: format!("{}/{}", config.endpoint, config.bucket),
            },
            // OSS 使用虚拟主机风格：`<scheme>://<bucket>.<endpoint host>/<object>`。
            StorageProvider::AliyunOss => {
                let endpoint = Url::parse(&config.endpoint)?;
                let host = endpoint
                    .host_str()
                    .ok_or_else(|| anyhow::anyhow!("SILVA_STORAGE_ENDPOINT has no host"))?;
                let port = endpoint
                    .port()
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                Self::Oss {
                    signer: reqsign_core::Signer::new(
                        reqsign_core::Context::new(),
                        reqsign_aliyun_oss::StaticCredentialProvider::new(
                            &config.access_key_id,
                            secret,
                        ),
                        reqsign_aliyun_oss::RequestSigner::new(&config.bucket)
                            .with_region(config.region.clone())
                            .with_signing_version(reqsign_aliyun_oss::SigningVersion::V4),
                    ),
                    base: format!("{}://{}.{host}{port}", endpoint.scheme(), config.bucket),
                }
            }
        })
    }

    fn base(&self) -> &str {
        match self {
            Self::R2 { base, .. } | Self::Oss { base, .. } => base,
        }
    }

    async fn sign(&self, parts: &mut http::request::Parts, expires_in: Duration) -> Result<()> {
        let result = match self {
            Self::R2 { signer, .. } => signer.sign(parts, Some(expires_in)).await,
            Self::Oss { signer, .. } => signer.sign(parts, Some(expires_in)).await,
        };
        result.map_err(|error| AppError::Internal(anyhow::anyhow!("presign failed: {error}")))
    }
}

impl OpendalStorage {
    pub fn new(config: &StorageConfig) -> anyhow::Result<Self> {
        // 关闭默认特性后，需要在构建存储客户端前显式注册 HTTP 传输。
        opendal::install_default();
        let operator = match config.provider {
            StorageProvider::R2 => Operator::new(
                services::S3::default()
                    .endpoint(&config.endpoint)
                    .bucket(&config.bucket)
                    .region(&config.region)
                    .root(&config.root)
                    .access_key_id(&config.access_key_id)
                    .secret_access_key(config.access_key_secret.expose())
                    .disable_config_load()
                    .disable_ec2_metadata(),
            )?,
            StorageProvider::AliyunOss => Operator::new(
                services::Oss::default()
                    .endpoint(&config.endpoint)
                    .bucket(&config.bucket)
                    .root(&config.root)
                    .access_key_id(&config.access_key_id)
                    .access_key_secret(config.access_key_secret.expose()),
            )?,
        };
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(UPLOAD_TIMEOUT)
                .build()?,
            operator: operator
                .layer(TimeoutLayer::new().with_timeout(OPERATION_TIMEOUT))
                .layer(RetryLayer::new().with_max_times(3).with_jitter()),
            signer: ObjectSigner::new(config)?,
            config: config.clone(),
        })
    }
}

fn storage_error(operation: &'static str) -> impl FnOnce(opendal::Error) -> AppError {
    move |error| {
        AppError::Internal(anyhow::anyhow!(
            "object storage {operation} failed: {error}"
        ))
    }
}

#[async_trait]
impl ObjectStorage for OpendalStorage {
    async fn put_stream(
        &self,
        key: &str,
        content_type: &str,
        length: u64,
        body: silva_application::ports::ByteStream,
    ) -> Result<()> {
        let url = format!("{}/{}", self.signer.base(), self.config.object_path(key));
        let condition = match self.config.provider {
            StorageProvider::R2 => ("if-none-match", "*"),
            StorageProvider::AliyunOss => ("x-oss-forbid-overwrite", "true"),
        };
        let (mut parts, ()) = http::Request::put(url)
            .header(condition.0, condition.1)
            .header(http::header::CONTENT_TYPE, content_type)
            .header(http::header::CONTENT_LENGTH, length)
            .body(())
            .map_err(|e| AppError::Internal(e.into()))?
            .into_parts();
        self.signer.sign(&mut parts, UPLOAD_TIMEOUT).await?;
        let response = self
            .client
            .put(parts.uri.to_string())
            .headers(parts.headers)
            .body(reqwest::Body::wrap_stream(body))
            .send()
            .await
            .map_err(|e| AppError::Internal(e.without_url().into()))?;
        if !response.status().is_success() {
            return Err(AppError::Internal(anyhow::anyhow!(
                "streaming object PUT returned {}",
                response.status()
            )));
        }
        Ok(())
    }

    async fn view_url(&self, key: &str, ttl: Duration) -> Result<String> {
        let url = format!("{}/{}", self.signer.base(), self.config.object_path(key));
        let (mut parts, ()) = http::Request::get(url)
            .body(())
            .map_err(|e| AppError::Internal(e.into()))?
            .into_parts();
        self.signer.sign(&mut parts, ttl).await?;
        Ok(parts.uri.to_string())
    }

    #[tracing::instrument(name = "storage.read", skip_all, fields(otel.kind = "client"))]
    async fn read(&self, key: &str, max_bytes: u64) -> Result<StoredObject> {
        let size = match self.operator.stat(key).await {
            Ok(metadata) => metadata.content_length(),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(StoredObject::Missing),
            Err(error) => return Err(storage_error("stat")(error)),
        };
        if size > max_bytes {
            return Ok(StoredObject::TooLarge { size });
        }
        if size == 0 {
            return Ok(StoredObject::Bytes(Vec::new()));
        }
        // 按 stat 得到的大小限定范围，客户端在两次请求之间改写对象也不会读取超过上限的字节。
        match self.operator.read_with(key).range(0..size).await {
            Ok(buffer) => Ok(StoredObject::Bytes(buffer.to_vec())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(StoredObject::Missing),
            Err(error) => Err(storage_error("read")(error)),
        }
    }

    #[tracing::instrument(name = "storage.stream", skip_all, fields(otel.kind = "client"))]
    async fn stream(&self, key: &str, max_bytes: u64) -> Result<StreamedObject> {
        let size = match self.operator.stat(key).await {
            Ok(metadata) => metadata.content_length(),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(StreamedObject::Missing);
            }
            Err(error) => return Err(storage_error("stat")(error)),
        };
        if size > max_bytes {
            return Ok(StreamedObject::TooLarge { size });
        }
        let reader = match self.operator.reader(key).await {
            Ok(reader) => reader,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(StreamedObject::Missing);
            }
            Err(error) => return Err(storage_error("open")(error)),
        };
        let body = reader
            .into_bytes_stream(0..size)
            .await
            .map_err(storage_error("open"))?;
        Ok(StreamedObject::Stream {
            size,
            body: Box::pin(body),
        })
    }

    #[tracing::instrument(name = "storage.put_if_absent", skip_all, fields(otel.kind = "client"))]
    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<PutOutcome> {
        match self
            .operator
            .write_with(key, bytes)
            .if_not_exists(true)
            .content_type(content_type)
            .await
        {
            Ok(_) => Ok(PutOutcome::Created),
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => {
                Ok(PutOutcome::AlreadyExists)
            }
            Err(error) => Err(storage_error("write")(error)),
        }
    }

    #[tracing::instrument(name = "storage.delete", skip_all, fields(otel.kind = "client"))]
    async fn delete(&self, key: &str) -> Result<()> {
        match self.operator.delete(key).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error("delete")(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silva_shared::config::StorageSecret;

    fn config(provider: StorageProvider, endpoint: &str, region: &str) -> StorageConfig {
        StorageConfig {
            provider,
            endpoint: endpoint.into(),
            bucket: "silva-media".into(),
            region: region.into(),
            root: "/silva/test/".into(),
            access_key_id: "AKIDEXAMPLE".into(),
            access_key_secret: StorageSecret::new("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY"),
        }
    }

    /// 查看地址绑定指定对象及有效期，仅凭链接即可发起 GET。
    #[tokio::test]
    async fn signed_view_scopes_object_and_expiry() {
        for (provider, endpoint, region, parameter) in [
            (
                StorageProvider::R2,
                "https://account.r2.cloudflarestorage.com",
                "auto",
                "X-Amz-Expires",
            ),
            (
                StorageProvider::AliyunOss,
                "https://oss-cn-hangzhou.aliyuncs.com",
                "cn-hangzhou",
                "x-oss-expires",
            ),
        ] {
            let storage = OpendalStorage::new(&config(provider, endpoint, region)).unwrap();
            let url = Url::parse(
                &storage
                    .view_url("media/abc", Duration::from_secs(900))
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert!(url.path().ends_with("/silva/test/media/abc"));
            assert!(
                url.query_pairs()
                    .any(|(key, value)| key.eq_ignore_ascii_case(parameter) && value == "900")
            );
        }
    }
}
