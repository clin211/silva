#![allow(dead_code)]
//! 用内存存储验证上传的真实字节、长度和条件写入。

use async_trait::async_trait;
use silva_application::ports::{ObjectStorage, PutOutcome, StoredObject, StreamedObject};
use silva_shared::error::Result as AppResult;
use std::{collections::HashMap, sync::Mutex, time::Duration};

/// 向实际 HTTP 路由发送分块图片，文件头故意拆开。
pub async fn post_image(
    app: &axum::Router,
    token: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
    declared: usize,
) -> (
    axum::http::StatusCode,
    axum::http::HeaderMap,
    serde_json::Value,
) {
    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Method, Request, header},
    };
    use tower::ServiceExt;
    let path = format!("/api/v1/admin/media?filename={filename}");
    let chunks: Vec<_> = bytes
        .chunks(7)
        .map(|c| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(c)))
        .collect();
    let response = app
        .clone()
        .oneshot(
            Request::post(&path)
                .extension(ConnectInfo(
                    "192.0.2.10:40000".parse::<std::net::SocketAddr>().unwrap(),
                ))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, mime)
                .header(header::CONTENT_LENGTH, declared)
                .body(Body::from_stream(futures_util::stream::iter(chunks)))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    crate::support::assert_response(&Method::POST, &path, status, &headers, &body);
    (status, headers, serde_json::from_slice(&body).unwrap())
}

/// 测试存储只在完整接收对象后提交。
#[derive(Default)]
pub struct MemoryStorage {
    pub objects: Mutex<HashMap<String, Vec<u8>>>,
    /// 删除时总是失败的键，用于模拟存储故障。
    pub failing_deletes: Mutex<Vec<String>>,
}

impl MemoryStorage {
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.objects.lock().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }
}

#[async_trait]
impl ObjectStorage for MemoryStorage {
    async fn put_stream(
        &self,
        key: &str,
        mime: &str,
        length: u64,
        mut body: silva_application::ports::ByteStream,
    ) -> AppResult<()> {
        use futures_util::StreamExt;
        let mut bytes = Vec::new();
        while let Some(chunk) = body.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(bytes.len() as u64, length);
        assert_eq!(
            self.put_if_absent(key, bytes, mime).await?,
            PutOutcome::Created
        );
        Ok(())
    }
    async fn view_url(&self, key: &str, _: Duration) -> AppResult<String> {
        Ok(format!("https://storage.test/{key}?read=1"))
    }
    async fn read(&self, key: &str, max_bytes: u64) -> AppResult<StoredObject> {
        Ok(match self.objects.lock().unwrap().get(key) {
            None => StoredObject::Missing,
            Some(bytes) if bytes.len() as u64 > max_bytes => StoredObject::TooLarge {
                size: bytes.len() as u64,
            },
            Some(bytes) => StoredObject::Bytes(bytes.clone()),
        })
    }

    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Vec<u8>,
        _content_type: &str,
    ) -> AppResult<PutOutcome> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(key) {
            return Ok(PutOutcome::AlreadyExists);
        }
        objects.insert(key.to_owned(), bytes);
        Ok(PutOutcome::Created)
    }

    async fn stream(&self, key: &str, max_bytes: u64) -> AppResult<StreamedObject> {
        Ok(match self.objects.lock().unwrap().get(key) {
            None => StreamedObject::Missing,
            Some(bytes) if bytes.len() as u64 > max_bytes => StreamedObject::TooLarge {
                size: bytes.len() as u64,
            },
            // 分成两块传输，验证响应按流拼接。
            Some(bytes) => {
                let (head, tail) = bytes.split_at(bytes.len() / 2);
                StreamedObject::Stream {
                    size: bytes.len() as u64,
                    body: Box::pin(futures_util::stream::iter([
                        Ok(bytes::Bytes::copy_from_slice(head)),
                        Ok(bytes::Bytes::copy_from_slice(tail)),
                    ])),
                }
            }
        })
    }

    async fn delete(&self, key: &str) -> AppResult<()> {
        if self
            .failing_deletes
            .lock()
            .unwrap()
            .iter()
            .any(|k| k == key)
        {
            return Err(silva_shared::error::AppError::Internal(anyhow_error()));
        }
        self.objects.lock().unwrap().remove(key);
        Ok(())
    }
}

fn anyhow_error() -> anyhow::Error {
    anyhow::anyhow!("simulated storage delete failure")
}
