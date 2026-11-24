//! 连接真实 R2 或 OSS bucket 的冒烟测试，验证签名直传（含大小绑定）、条件写入、读取与删除。
//! 读取与运行时相同的 `SILVA_STORAGE_*` 环境变量，通过
//! `cargo test -p silva-infrastructure --test object_storage_live -- --ignored` 显式执行。
//! 测试对象写在 `smoke/` 前缀下并在结束时删除。

use silva_application::ports::{ObjectStorage, PutOutcome, StoredObject};
use silva_infrastructure::object_storage::OpendalStorage;
use silva_shared::{config::StorageConfig, ids};
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "需要通过 SILVA_STORAGE_* 环境变量连接真实的 R2 或 OSS bucket"]
async fn presigned_upload_conditional_write_and_delete() {
    let config = StorageConfig::from_env(false)
        .unwrap()
        .expect("set SILVA_STORAGE_PROVIDER and related variables; live tests never silently skip");
    let storage = Arc::new(OpendalStorage::new(&config).unwrap());
    let id = ids::new_id();
    let upload_key = format!("smoke/uploads/{id}");
    let formal_key = format!("smoke/media/{id}");
    let body = format!("silva storage smoke {id}").into_bytes();

    // 与浏览器相同：只用签名地址和下发的请求头直传，不带服务端凭据。
    let upload = storage
        .presign_put(
            &upload_key,
            "image/png",
            body.len() as u64,
            Duration::from_secs(300),
        )
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let put = |payload: Vec<u8>| {
        let mut request = client.request(upload.method.parse().unwrap(), &upload.url);
        for (name, value) in &upload.headers {
            request = request.header(name, value);
        }
        request.body(payload).send()
    };
    // 签名绑定了 Content-Length：与声明大小不同的请求体必须被存储服务拒绝。
    let mut oversized = body.clone();
    oversized.extend_from_slice(&[0; 1024]);
    let rejected = put(oversized).await.unwrap().status();
    assert!(
        rejected.is_client_error(),
        "大小与签名不符的直传应被拒绝，实际为 {rejected}"
    );
    let response = put(body.clone()).await.unwrap();
    let status = response.status();
    assert!(
        status.is_success(),
        "直传失败：{status} {}",
        response.text().await.unwrap_or_default()
    );

    // 断言在独立任务中执行，失败时仍会清理测试对象。
    let checks = {
        let (storage, upload_key, formal_key, body) = (
            storage.clone(),
            upload_key.clone(),
            formal_key.clone(),
            body.clone(),
        );
        tokio::spawn(async move {
            assert_eq!(
                storage.read(&upload_key, 1024).await.unwrap(),
                StoredObject::Bytes(body.clone())
            );
            assert_eq!(
                storage.read(&upload_key, 4).await.unwrap(),
                StoredObject::TooLarge {
                    size: body.len() as u64
                }
            );
            assert_eq!(
                storage
                    .put_if_absent(&formal_key, body.clone(), "image/png")
                    .await
                    .unwrap(),
                PutOutcome::Created
            );
            assert_eq!(
                storage
                    .put_if_absent(&formal_key, b"overwrite".to_vec(), "image/png")
                    .await
                    .unwrap(),
                PutOutcome::AlreadyExists,
                "正式对象不可覆盖；OSS bucket 开启版本控制时该保证失效"
            );
            assert_eq!(
                storage.read(&formal_key, 1024).await.unwrap(),
                StoredObject::Bytes(body.clone())
            );
        })
        .await
    };

    for key in [&upload_key, &formal_key] {
        storage.delete(key).await.unwrap();
        assert_eq!(
            storage.read(key, 1024).await.unwrap(),
            StoredObject::Missing
        );
    }
    // 删除不存在的对象视为成功。
    storage.delete(&upload_key).await.unwrap();
    checks.expect("live storage assertions failed");
}
