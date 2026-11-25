//! 使用独立的 smoke 键验证真实对象存储的流式写入、只读链接与不可覆盖约束。
use silva_application::ports::{ObjectStorage, StoredObject};
use silva_infrastructure::object_storage::OpendalStorage;
use silva_shared::{config::StorageConfig, ids};
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "需要 SILVA_STORAGE_* 配置的专用测试前缀"]
async fn stream_upload_and_open_view_url() {
    let config = StorageConfig::from_env(false)
        .unwrap()
        .expect("missing storage configuration");
    let storage = Arc::new(OpendalStorage::new(&config).unwrap());
    let key = format!("smoke/media/{}", ids::new_id());
    let checks = {
        let storage = storage.clone();
        let key = key.clone();
        tokio::spawn(async move {
            let mut buffer = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(3, 2)
                .write_to(&mut buffer, image::ImageFormat::Png)
                .unwrap();
            let bytes = bytes::Bytes::from(buffer.into_inner());
            let put = || {
                storage.put_stream(
                    &key,
                    "image/png",
                    bytes.len() as u64,
                    Box::pin(futures_util::stream::iter([
                        Ok(bytes.slice(..7)),
                        Ok(bytes.slice(7..)),
                    ])),
                )
            };
            put().await.unwrap();
            assert!(put().await.is_err());
            let url = storage
                .view_url(&key, Duration::from_secs(60))
                .await
                .unwrap();
            let response = reqwest::get(url)
                .await
                .map_err(|e| e.without_url())
                .unwrap();
            assert!(response.status().is_success());
            assert_eq!(response.headers()["content-type"], "image/png");
            assert_eq!(response.bytes().await.unwrap().as_ref(), bytes);
        })
        .await
    };
    storage.delete(&key).await.unwrap();
    assert_eq!(
        storage.read(&key, 1024).await.unwrap(),
        StoredObject::Missing
    );
    checks.expect("storage checks failed");
}
