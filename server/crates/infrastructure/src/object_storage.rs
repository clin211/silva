//! 基于 Apache OpenDAL 的私有对象存储适配器，支持 Cloudflare R2 与阿里云 OSS。
//!
//! - R2 走 S3 兼容接口与 AWS SigV4，条件写入使用 `If-None-Match: *`。
//! - OSS 走原生接口与签名，条件写入使用 `x-oss-forbid-overwrite: true`；bucket 开启版本控制时
//!   OSS 会忽略该请求头，因此媒体 bucket 不应开启版本控制。
//!
//! 浏览器直传地址不经过 OpenDAL：OpenDAL 预签名时不携带 `Content-Length`，OSS 还默认使用
//! 不覆盖该头的 V1 签名。这里直接用 reqsign 签名，R2 使用 SigV4 查询签名，OSS 使用 V4 查询签名，
//! 把 `content-type` 和 `content-length` 都纳入签名，客户端只能上传与声明大小完全一致的文件。
//!
//! 只使用配置中的显式密钥，不读取运行环境的默认凭据链或云主机元数据。

use async_trait::async_trait;
use opendal::{
    ErrorKind, Operator,
    layers::{RetryLayer, TimeoutLayer},
    services,
};
use silva_application::ports::{
    ObjectStorage, PresignedUpload, PutOutcome, StoredObject, StreamedObject,
};
use silva_shared::{
    config::{StorageConfig, StorageProvider},
    error::{AppError, Result},
};
use std::time::Duration;
use url::Url;

/// 单次存储操作的超时时间；10 MiB 以内的对象足够完成读写。
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
/// 浏览器不允许脚本设置的请求头，签名结果中出现时不下发给客户端，由浏览器自动携带。
const BROWSER_MANAGED_HEADERS: [&str; 2] = ["host", "content-length"];

pub struct OpendalStorage {
    operator: Operator,
    signer: UploadSigner,
    config: StorageConfig,
}

/// 浏览器直传地址的签名器；`base` 为对象路径之前的地址部分。
enum UploadSigner {
    R2 {
        signer: reqsign_core::Signer<reqsign_aws_v4::Credential>,
        base: String,
    },
    Oss {
        signer: reqsign_core::Signer<reqsign_aliyun_oss::Credential>,
        base: String,
    },
}

impl UploadSigner {
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
            operator: operator
                .layer(TimeoutLayer::new().with_timeout(OPERATION_TIMEOUT))
                .layer(RetryLayer::new().with_max_times(3).with_jitter()),
            signer: UploadSigner::new(config)?,
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
    #[tracing::instrument(name = "storage.presign_put", skip_all, fields(otel.kind = "client"))]
    async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: u64,
        expires_in: Duration,
    ) -> Result<PresignedUpload> {
        let url = format!("{}/{}", self.signer.base(), self.config.object_path(key));
        let (mut parts, ()) = http::Request::put(url)
            .header(http::header::CONTENT_TYPE, content_type)
            .header(http::header::CONTENT_LENGTH, content_length)
            .body(())
            .map_err(|error| AppError::Internal(error.into()))?
            .into_parts();
        self.signer.sign(&mut parts, expires_in).await?;
        // content-length 已签名，但浏览器按请求体自动设置，不允许脚本指定。
        let headers = parts
            .headers
            .iter()
            .filter(|(name, _)| !BROWSER_MANAGED_HEADERS.contains(&name.as_str()))
            .map(|(name, value)| {
                value
                    .to_str()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
                    .map_err(|error| AppError::Internal(error.into()))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PresignedUpload {
            method: parts.method.as_str().to_owned(),
            url: parts.uri.to_string(),
            headers,
        })
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

    /// 签名在本地完成，不访问网络：直传地址指向根前缀下的临时键，签名覆盖
    /// `content-type` 与 `content-length`，并只下发浏览器可以设置的请求头。
    #[tokio::test]
    async fn presigned_put_signs_content_type_and_length() {
        for (provider, endpoint, region, host, path, signed) in [
            (
                StorageProvider::R2,
                "https://account.r2.cloudflarestorage.com",
                "auto",
                "account.r2.cloudflarestorage.com",
                "/silva-media/silva/test/uploads/abc",
                ("X-Amz-SignedHeaders", "content-length;content-type;host"),
            ),
            (
                StorageProvider::AliyunOss,
                "https://oss-cn-hangzhou.aliyuncs.com",
                "cn-hangzhou",
                "silva-media.oss-cn-hangzhou.aliyuncs.com",
                "/silva/test/uploads/abc",
                ("x-oss-additional-headers", "content-length;host"),
            ),
        ] {
            let storage = OpendalStorage::new(&config(provider, endpoint, region)).unwrap();
            let upload = storage
                .presign_put("uploads/abc", "image/png", 2048, Duration::from_secs(900))
                .await
                .unwrap();
            assert_eq!(upload.method, "PUT", "{provider:?}");
            let url = Url::parse(&upload.url).unwrap();
            assert_eq!(url.host_str(), Some(host), "{provider:?}");
            assert_eq!(url.path(), path, "{provider:?}");
            let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
            let value = |name: &str| {
                query
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value.as_str())
            };
            assert_eq!(value(signed.0), Some(signed.1), "{provider:?}: {query:?}");
            if provider == StorageProvider::AliyunOss {
                assert_eq!(value("x-oss-signature-version"), Some("OSS4-HMAC-SHA256"));
            }
            assert!(
                !upload.url.contains("wJalrXUtnFEMI"),
                "{provider:?}: 地址不得包含密钥"
            );
            assert_eq!(
                upload.headers,
                vec![("content-type".to_owned(), "image/png".to_owned())],
                "{provider:?}: 只下发 content-type"
            );
        }
    }
}
