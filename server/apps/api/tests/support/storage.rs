#![allow(dead_code)]
//! 测试用的内存对象存储：直传地址形如 `https://storage.test/<key>?signature=test`，
//! 与真实存储一致，直传字节数必须等于签名时绑定的长度。

use async_trait::async_trait;
use silva_application::ports::{
    ObjectStorage, PresignedUpload, PutOutcome, StoredObject, StreamedObject,
};
use silva_shared::error::Result as AppResult;
use std::{collections::HashMap, sync::Mutex, time::Duration};

pub const STORAGE_ORIGIN: &str = "https://storage.test/";

/// 内存对象存储：签发的直传地址形如 `https://storage.test/<key>?signature=test`。
/// 与真实存储一致，直传的字节数必须等于签名时绑定的长度。
#[derive(Default)]
pub struct MemoryStorage {
    pub objects: Mutex<HashMap<String, Vec<u8>>>,
    signed_lengths: Mutex<HashMap<String, u64>>,
    /// 删除时总是失败的键，用于模拟存储故障。
    pub failing_deletes: Mutex<Vec<String>>,
}

impl MemoryStorage {
    /// 模拟浏览器按直传地址 PUT 文件；字节数与签名不符时返回 false 且不写入。
    pub fn upload(&self, url: &str, bytes: &[u8]) -> bool {
        let key = url
            .strip_prefix(STORAGE_ORIGIN)
            .and_then(|rest| rest.split('?').next())
            .expect("直传地址指向测试存储");
        if self.signed_lengths.lock().unwrap().get(key) != Some(&(bytes.len() as u64)) {
            return false;
        }
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_owned(), bytes.to_vec());
        true
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.objects.lock().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }
}

#[async_trait]
impl ObjectStorage for MemoryStorage {
    async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: u64,
        _expires_in: Duration,
    ) -> AppResult<PresignedUpload> {
        self.signed_lengths
            .lock()
            .unwrap()
            .insert(key.to_owned(), content_length);
        Ok(PresignedUpload {
            method: "PUT".into(),
            url: format!("{STORAGE_ORIGIN}{key}?signature=test"),
            headers: vec![("content-type".into(), content_type.into())],
        })
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
