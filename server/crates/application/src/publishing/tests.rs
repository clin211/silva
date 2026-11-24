//! 发布流水线的单元测试：内存仓储、内存对象存储和可控的构建器。

use super::*;
use crate::ports::{
    ActiveSite, BuiltFile, ImageSize, ObjectStorage, PresignedUpload, RenderedMarkdown,
    StoredObject, StreamedObject,
};
use async_trait::async_trait;
use silva_domain::site::{PublishJob, PublishJobFilter, SiteSnapshot, SnapshotPost};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

/// 记录的一次切换：清单键、产物前缀与文章修订映射。
type ActivationRecord = (String, String, Vec<(Uuid, Uuid)>);

/// 仓储行为可配置的假实现，只实现流水线用到的方法。
#[derive(Default)]
struct FakeRepository {
    job: Mutex<Option<ClaimedJob>>,
    still_desired: AtomicBool,
    lease_valid: AtomicBool,
    activated: Mutex<Vec<ActivationRecord>>,
    failed: Mutex<Vec<String>>,
    superseded: AtomicBool,
    posts: Vec<SnapshotPost>,
}

impl FakeRepository {
    fn new(posts: Vec<SnapshotPost>) -> Self {
        let repository = Self {
            posts,
            ..Self::default()
        };
        *repository.job.lock().unwrap() = Some(ClaimedJob {
            id: Uuid::now_v7(),
            generation: 7,
            attempt: 2,
            lease_token: Uuid::now_v7(),
        });
        repository.still_desired.store(true, Ordering::SeqCst);
        repository.lease_valid.store(true, Ordering::SeqCst);
        repository
    }
}

#[async_trait]
impl PublishingRepository for FakeRepository {
    async fn jobs(&self, _: &PageRequest, _: &PublishJobFilter) -> Result<Page<PublishJob>> {
        unimplemented!()
    }
    async fn job(&self, _: Uuid) -> Result<Option<PublishJob>> {
        unimplemented!()
    }
    async fn request_build(&self, _: &str, _: Option<&ContentActor<'_>>) -> Result<SiteTarget> {
        unimplemented!()
    }
    async fn retry(&self, _: Uuid, _: &ContentActor<'_>) -> Result<PublishJob> {
        unimplemented!()
    }
    async fn claim(&self, _: Duration, _: i32) -> Result<Option<ClaimedJob>> {
        Ok(self.job.lock().unwrap().take())
    }
    async fn renew(&self, _: &ClaimedJob, _: Duration) -> Result<bool> {
        Ok(self.lease_valid.load(Ordering::SeqCst))
    }
    async fn export(&self, _: &ClaimedJob) -> Result<Option<ExportedSite>> {
        if !self.still_desired.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(Some(ExportedSite {
            posts: self.posts.clone(),
            categories: vec![],
            tags: vec![],
            profile: None,
            media: vec![],
        }))
    }
    async fn activate(
        &self,
        _: &ClaimedJob,
        manifest_key: &str,
        artifact_prefix: &str,
        posts: &[(Uuid, Uuid)],
    ) -> Result<Activation> {
        self.activated.lock().unwrap().push((
            manifest_key.to_owned(),
            artifact_prefix.to_owned(),
            posts.to_vec(),
        ));
        Ok(Activation::Activated)
    }
    async fn supersede(&self, _: &ClaimedJob) -> Result<()> {
        self.superseded.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn fail(&self, _: &ClaimedJob, error: &str, _: i32, _: Duration) -> Result<()> {
        self.failed.lock().unwrap().push(error.to_owned());
        Ok(())
    }
    async fn active_site(&self) -> Result<Option<ActiveSite>> {
        unimplemented!()
    }
}

#[derive(Default)]
struct MemoryStorage {
    objects: Mutex<HashMap<String, (Vec<u8>, String)>>,
}

#[async_trait]
impl ObjectStorage for MemoryStorage {
    async fn presign_put(&self, _: &str, _: &str, _: u64, _: Duration) -> Result<PresignedUpload> {
        unimplemented!()
    }
    async fn read(&self, key: &str, _: u64) -> Result<StoredObject> {
        Ok(match self.objects.lock().unwrap().get(key) {
            Some((bytes, _)) => StoredObject::Bytes(bytes.clone()),
            None => StoredObject::Missing,
        })
    }
    async fn stream(&self, _: &str, _: u64) -> Result<StreamedObject> {
        unimplemented!()
    }
    async fn put_if_absent(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<PutOutcome> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(key) {
            return Ok(PutOutcome::AlreadyExists);
        }
        objects.insert(key.to_owned(), (bytes, content_type.to_owned()));
        Ok(PutOutcome::Created)
    }
    async fn delete(&self, _: &str) -> Result<()> {
        Ok(())
    }
}

/// 构建器：校验收到的快照摘要，按配置返回文件、失败或长时间挂起。
#[derive(Default)]
struct FakeBuilder {
    files: Vec<(&'static str, &'static str)>,
    fail: bool,
    hang: bool,
    received: Mutex<Option<(i64, SiteSnapshot)>>,
}

#[async_trait]
impl SiteBuilder for FakeBuilder {
    async fn build(
        &self,
        snapshot: &[u8],
        sha256: &str,
        generation: i64,
    ) -> Result<Vec<BuiltFile>> {
        assert_eq!(sha256_hex(snapshot), sha256);
        *self.received.lock().unwrap() =
            Some((generation, serde_json::from_slice(snapshot).unwrap()));
        if self.hang {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        if self.fail {
            return Err(AppError::Internal(anyhow::anyhow!(
                "astro exited with status 1"
            )));
        }
        Ok(self
            .files
            .iter()
            .map(|(path, body)| BuiltFile {
                path: (*path).to_owned(),
                content_type: "text/html; charset=utf-8".into(),
                bytes: body.as_bytes().to_vec(),
            })
            .collect())
    }
}

struct PlainRenderer;

impl MarkdownRenderer for PlainRenderer {
    fn media_references(&self, _: &'static str, _: &str) -> Result<Vec<Uuid>> {
        Ok(vec![])
    }
    fn render(&self, markdown: &str, _: &HashMap<Uuid, ImageSize>) -> RenderedMarkdown {
        RenderedMarkdown {
            html: format!("<p>{markdown}</p>"),
            headings: vec![],
        }
    }
}

fn post() -> SnapshotPost {
    SnapshotPost {
        id: Uuid::now_v7(),
        revision_id: Uuid::now_v7(),
        slug: "hello".into(),
        title: "Hello".into(),
        excerpt: None,
        html: "<p>Hi</p>".into(),
        headings: vec![],
        categories: vec![],
        tags: vec![],
        featured: false,
        cover: None,
        seo_title: None,
        seo_description: None,
        published_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

const SITE: [(&str, &str); 3] = [
    ("index.html", "<html>home</html>"),
    ("404.html", "<html>missing</html>"),
    ("blog/hello/index.html", "<html>post</html>"),
];

fn service(
    repository: Arc<FakeRepository>,
    storage: Arc<MemoryStorage>,
    builder: FakeBuilder,
    lease: Duration,
) -> (PublishingService, Arc<FakeBuilder>) {
    let builder = Arc::new(builder);
    (
        PublishingService::new(
            repository,
            Some(storage),
            Some(builder.clone()),
            Arc::new(PlainRenderer),
            PublishSettings {
                lease,
                ..PublishSettings::default()
            },
        ),
        builder,
    )
}

#[tokio::test]
async fn successful_build_uploads_snapshot_artifacts_and_manifest_then_activates() {
    let post = post();
    let repository = Arc::new(FakeRepository::new(vec![post.clone()]));
    let storage = Arc::new(MemoryStorage::default());
    let (service, builder) = service(
        repository.clone(),
        storage.clone(),
        FakeBuilder {
            files: SITE.to_vec(),
            ..Default::default()
        },
        Duration::from_secs(60),
    );
    assert_eq!(
        service.run_once().await.unwrap(),
        Some(PublishOutcome::Activated { generation: 7 })
    );
    let (generation, snapshot) = builder.received.lock().unwrap().clone().unwrap();
    assert_eq!((generation, snapshot.generation), (7, 7));
    assert_eq!(snapshot.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snapshot.posts, vec![post.clone()]);

    let objects = storage.objects.lock().unwrap().clone();
    let snapshot_bytes = &objects["site/7/2/snapshot.json"].0;
    assert_eq!(
        objects["site/7/2/dist/blog/hello/index.html"].0,
        b"<html>post</html>"
    );
    let manifest: ArtifactManifest =
        serde_json::from_slice(&objects["site/7/2/manifest.json"].0).unwrap();
    assert_eq!(manifest.snapshot_sha256, sha256_hex(snapshot_bytes));
    assert_eq!(
        manifest
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["404.html", "blog/hello/index.html", "index.html"]
    );
    assert_eq!(manifest.files[2].sha256, sha256_hex(b"<html>home</html>"));

    let activated = repository.activated.lock().unwrap().clone();
    assert_eq!(
        activated.as_slice(),
        [(
            "site/7/2/manifest.json".to_owned(),
            "site/7/2/dist/".to_owned(),
            vec![(post.id, post.revision_id)]
        )]
    );
    // 没有任务可领取时返回 None。
    assert_eq!(service.run_once().await.unwrap(), None);
}

#[tokio::test]
async fn job_that_is_no_longer_desired_is_superseded_without_building() {
    let repository = Arc::new(FakeRepository::new(vec![]));
    repository.still_desired.store(false, Ordering::SeqCst);
    let storage = Arc::new(MemoryStorage::default());
    let (service, builder) = service(
        repository.clone(),
        storage.clone(),
        FakeBuilder::default(),
        Duration::from_secs(60),
    );
    assert_eq!(
        service.run_once().await.unwrap(),
        Some(PublishOutcome::Superseded { generation: 7 })
    );
    assert!(repository.superseded.load(Ordering::SeqCst));
    assert!(builder.received.lock().unwrap().is_none());
    assert!(storage.objects.lock().unwrap().is_empty());
}

#[tokio::test]
async fn build_failures_and_incomplete_output_are_recorded_without_activation() {
    for (builder, expected) in [
        (
            FakeBuilder {
                fail: true,
                ..Default::default()
            },
            "astro exited with status 1",
        ),
        (
            FakeBuilder {
                files: vec![("index.html", "<html></html>")],
                ..Default::default()
            },
            "did not produce 404.html",
        ),
    ] {
        let repository = Arc::new(FakeRepository::new(vec![]));
        let (service, _) = service(
            repository.clone(),
            Arc::new(MemoryStorage::default()),
            builder,
            Duration::from_secs(60),
        );
        let Some(PublishOutcome::Failed { generation, error }) = service.run_once().await.unwrap()
        else {
            panic!("期望构建失败");
        };
        assert_eq!(generation, 7);
        assert!(error.contains(expected), "{error}");
        assert_eq!(repository.failed.lock().unwrap().as_slice(), [error]);
        assert!(repository.activated.lock().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn losing_the_lease_abandons_the_build() {
    let repository = Arc::new(FakeRepository::new(vec![]));
    repository.lease_valid.store(false, Ordering::SeqCst);
    let (service, _) = service(
        repository.clone(),
        Arc::new(MemoryStorage::default()),
        FakeBuilder {
            hang: true,
            ..Default::default()
        },
        Duration::from_secs(30),
    );
    assert_eq!(
        service.run_once().await.unwrap(),
        Some(PublishOutcome::LeaseLost { generation: 7 })
    );
    assert!(
        repository.failed.lock().unwrap().is_empty(),
        "租约失效时不记录失败"
    );
    assert!(repository.activated.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unconfigured_builder_does_not_claim_jobs() {
    let repository = Arc::new(FakeRepository::new(vec![]));
    let service = PublishingService::new(
        repository.clone(),
        None,
        None,
        Arc::new(PlainRenderer),
        PublishSettings::default(),
    );
    assert!(!service.can_build());
    assert_eq!(service.run_once().await.unwrap(), None);
    assert!(repository.job.lock().unwrap().is_some(), "任务未被领取");
}

#[test]
fn retry_delay_doubles_and_is_capped() {
    let settings = PublishSettings::default();
    assert_eq!(settings.retry_delay(1), Duration::from_secs(30));
    assert_eq!(settings.retry_delay(2), Duration::from_secs(60));
    assert_eq!(settings.retry_delay(3), Duration::from_secs(120));
    assert_eq!(settings.retry_delay(10), Duration::from_secs(300));
}
