//! 站点发布：后台查询与重试发布任务，Worker 领取任务、导出快照、构建、上传产物并原子切换线上版本。
//!
//! 流程见后端技术文档 §7.2。构建和对象存储 I/O 不持有数据库锁；每一步之间与构建期间按租约
//! 续期，租约失效（超时被回收或被其他 Worker 接手）时立即放弃，不能覆盖后续尝试的结果。

use crate::ports::{
    Activation, ClaimedJob, ContentActor, ExportedSite, MarkdownRenderer, ObjectStorage,
    PublishingRepository, PutOutcome, RequestMeta, SiteBuilder,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use silva_domain::{
    identity::Principal,
    site::{
        ArtifactFile, ArtifactManifest, PublishJob, PublishJobFilter, SNAPSHOT_SCHEMA_VERSION,
        SiteSnapshot, SiteTarget, SnapshotExperience, SnapshotProfile, SnapshotTrending,
        attempt_prefix,
    },
};
use silva_shared::{
    error::{AppError, Result},
    pagination::{Page, PageRequest},
};
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use uuid::Uuid;

pub const JOBS_READ: &str = "jobs.read";
pub const JOBS_READ_OWN: &str = "jobs.read_own";
pub const PUBLISHING_RETRY: &str = "publishing.retry";

/// 构建必须产出的文件。
const REQUIRED_FILES: [&str; 2] = ["index.html", "404.html"];
/// 写入任务的失败原因的最大字节数。
const ERROR_MAX_BYTES: usize = 2000;

/// Worker 构建参数。
#[derive(Clone, Copy, Debug)]
pub struct PublishSettings {
    /// 租约时长；每三分之一租约续期一次。
    pub lease: Duration,
    /// 自动构建的最多次数（含首次）。
    pub max_attempts: i32,
    /// 首次失败后的重试间隔，之后每次翻倍，最长 5 分钟。
    pub retry_base: Duration,
}

impl Default for PublishSettings {
    fn default() -> Self {
        Self {
            lease: Duration::from_secs(120),
            max_attempts: 3,
            retry_base: Duration::from_secs(30),
        }
    }
}

impl PublishSettings {
    /// 第 `attempt` 次构建失败后的重试间隔：`retry_base * 2^(attempt-1)`，最长 5 分钟。
    pub fn retry_delay(&self, attempt: i32) -> Duration {
        let exponent = u32::try_from(attempt.saturating_sub(1)).unwrap_or(0).min(8);
        (self.retry_base * 2u32.pow(exponent)).min(Duration::from_secs(300))
    }
}

/// 一次 Worker 处理的结果。
#[derive(Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    /// 构建成功并已上线。
    Activated { generation: i64 },
    /// 处理期间已有更新的目标，未上线。
    Superseded { generation: i64 },
    /// 租约失效，结果已丢弃。
    LeaseLost { generation: i64 },
    /// 构建失败，已记录原因；未用尽重试时重新排队。
    Failed { generation: i64, error: String },
}

pub struct PublishingService {
    repository: Arc<dyn PublishingRepository>,
    storage: Option<Arc<dyn ObjectStorage>>,
    builder: Option<Arc<dyn SiteBuilder>>,
    renderer: Arc<dyn MarkdownRenderer>,
    settings: PublishSettings,
}

impl PublishingService {
    pub fn new(
        repository: Arc<dyn PublishingRepository>,
        storage: Option<Arc<dyn ObjectStorage>>,
        builder: Option<Arc<dyn SiteBuilder>>,
        renderer: Arc<dyn MarkdownRenderer>,
        settings: PublishSettings,
    ) -> Self {
        Self {
            repository,
            storage,
            builder,
            renderer,
            settings,
        }
    }

    /// 对象存储与构建器都已配置时 Worker 才处理发布任务。
    pub fn can_build(&self) -> bool {
        self.storage.is_some() && self.builder.is_some()
    }

    #[tracing::instrument(name = "publishing.jobs", skip_all)]
    pub async fn jobs(
        &self,
        principal: &Principal,
        page: &PageRequest,
        filter: PublishJobFilter,
    ) -> Result<Page<PublishJob>> {
        page.validate()?;
        let filter = if principal.require(JOBS_READ).is_ok() {
            filter
        } else {
            principal.require(JOBS_READ_OWN)?;
            PublishJobFilter {
                requested_by: Some(principal.user.id),
                ..filter
            }
        };
        self.repository.jobs(page, &filter).await
    }

    #[tracing::instrument(name = "publishing.job", skip_all)]
    pub async fn job(&self, principal: &Principal, id: Uuid) -> Result<PublishJob> {
        let all = principal.require(JOBS_READ).is_ok();
        if !all {
            principal.require(JOBS_READ_OWN)?;
        }
        let job = self.repository.job(id).await?.ok_or(AppError::NotFound)?;
        if all
            || job
                .requested_by
                .as_ref()
                .is_some_and(|user| user.id == principal.user.id)
        {
            Ok(job)
        } else {
            Err(AppError::NotFound)
        }
    }

    /// 管理员手动登记全站重建。
    #[tracing::instrument(name = "publishing.request_build", skip_all)]
    pub async fn request_build(
        &self,
        principal: &Principal,
        meta: &RequestMeta,
    ) -> Result<SiteTarget> {
        principal.require(PUBLISHING_RETRY)?;
        self.repository
            .request_build(
                "manual",
                Some(&ContentActor {
                    user_id: principal.user.id,
                    meta,
                }),
            )
            .await
    }

    /// 首次部署由 CLI 登记初始站点目标。
    #[tracing::instrument(name = "publishing.bootstrap", skip_all)]
    pub async fn bootstrap(&self) -> Result<SiteTarget> {
        self.repository.request_build("bootstrap", None).await
    }

    /// 重新排队仍为最新目标的失败任务。
    #[tracing::instrument(name = "publishing.retry", skip_all)]
    pub async fn retry(
        &self,
        principal: &Principal,
        id: Uuid,
        meta: &RequestMeta,
    ) -> Result<PublishJob> {
        principal.require(PUBLISHING_RETRY)?;
        self.repository
            .retry(
                id,
                &ContentActor {
                    user_id: principal.user.id,
                    meta,
                },
            )
            .await
    }

    /// 领取并处理一个发布任务；没有可处理的任务或未配置构建时返回 None。
    #[tracing::instrument(name = "publishing.run_once", skip_all)]
    pub async fn run_once(&self) -> Result<Option<PublishOutcome>> {
        let (Some(storage), Some(builder)) = (self.storage.as_deref(), self.builder.as_deref())
        else {
            return Ok(None);
        };
        let Some(job) = self
            .repository
            .claim(self.settings.lease, self.settings.max_attempts)
            .await?
        else {
            return Ok(None);
        };
        let generation = job.generation;
        tracing::info!(generation, attempt = job.attempt, "site build started");
        let outcome = match self
            .with_lease(&job, self.process(&job, storage, builder))
            .await
        {
            Ok(Some(outcome)) => outcome,
            Ok(None) => PublishOutcome::LeaseLost { generation },
            Err(error) => {
                let message = truncate(&format!("{error:#}"), ERROR_MAX_BYTES);
                self.repository
                    .fail(
                        &job,
                        &message,
                        self.settings.max_attempts,
                        self.settings.retry_delay(job.attempt),
                    )
                    .await?;
                PublishOutcome::Failed {
                    generation,
                    error: message,
                }
            }
        };
        match &outcome {
            PublishOutcome::Activated { .. } => {
                tracing::info!(generation, "site generation activated")
            }
            PublishOutcome::Failed { error, .. } => {
                tracing::warn!(generation, %error, "site build failed")
            }
            other => {
                tracing::info!(generation, outcome = ?other, "site build ended without activation")
            }
        }
        Ok(Some(outcome))
    }

    /// 在执行 `work` 的同时按租约续期；续期失败时放弃 `work` 并返回 None。
    async fn with_lease<T>(
        &self,
        job: &ClaimedJob,
        work: impl Future<Output = Result<T>>,
    ) -> Result<Option<T>> {
        let period = self.settings.lease / 3;
        let renew = async {
            let mut tick = tokio::time::interval(period);
            tick.tick().await;
            loop {
                tick.tick().await;
                match self.repository.renew(job, self.settings.lease).await {
                    Ok(true) => {}
                    Ok(false) => return Ok::<(), AppError>(()),
                    // 续期请求失败时继续执行，下一次续期再判断；租约到期后由 activate 拒绝。
                    Err(error) => tracing::warn!(%error, "publish lease renewal failed"),
                }
            }
        };
        tokio::select! {
            result = work => result.map(Some),
            lost = renew => lost.map(|()| None),
        }
    }

    async fn process(
        &self,
        job: &ClaimedJob,
        storage: &dyn ObjectStorage,
        builder: &dyn SiteBuilder,
    ) -> Result<PublishOutcome> {
        let generation = job.generation;
        let Some(exported) = self.repository.export(job).await? else {
            self.repository.supersede(job).await?;
            return Ok(PublishOutcome::Superseded { generation });
        };
        let mapping: Vec<(Uuid, Uuid)> = exported
            .posts
            .iter()
            .map(|post| (post.id, post.revision_id))
            .collect();
        let snapshot = self.snapshot(generation, exported);
        let bytes =
            serde_json::to_vec(&snapshot).map_err(|error| AppError::Internal(error.into()))?;
        let snapshot_sha256 = sha256_hex(&bytes);
        let prefix = attempt_prefix(generation, job.attempt);
        put_new(
            storage,
            &format!("{prefix}snapshot.json"),
            bytes.clone(),
            "application/json",
        )
        .await?;

        let files = builder.build(&bytes, &snapshot_sha256, generation).await?;
        for required in REQUIRED_FILES {
            if !files.iter().any(|file| file.path == required) {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "site build did not produce {required}"
                )));
            }
        }
        let artifact_prefix = format!("{prefix}dist/");
        let mut manifest = ArtifactManifest {
            generation,
            attempt: job.attempt,
            snapshot_sha256,
            files: Vec::with_capacity(files.len()),
        };
        for file in files {
            manifest.files.push(ArtifactFile {
                size: file.bytes.len() as u64,
                sha256: sha256_hex(&file.bytes),
                content_type: file.content_type.clone(),
                path: file.path.clone(),
            });
            put_new(
                storage,
                &format!("{artifact_prefix}{}", file.path),
                file.bytes,
                &file.content_type,
            )
            .await?;
        }
        manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest_key = format!("{prefix}manifest.json");
        let manifest_bytes =
            serde_json::to_vec(&manifest).map_err(|error| AppError::Internal(error.into()))?;
        put_new(storage, &manifest_key, manifest_bytes, "application/json").await?;

        Ok(
            match self
                .repository
                .activate(job, &manifest_key, &artifact_prefix, &mapping)
                .await?
            {
                Activation::Activated => PublishOutcome::Activated { generation },
                Activation::Superseded => PublishOutcome::Superseded { generation },
                Activation::LeaseLost => PublishOutcome::LeaseLost { generation },
            },
        )
    }

    /// 组装快照；个人资料的 Markdown 在这里渲染。
    fn snapshot(&self, generation: i64, site: ExportedSite) -> SiteSnapshot {
        let render = |markdown: Option<String>| {
            markdown
                .filter(|value| !value.trim().is_empty())
                .map(|value| self.renderer.render(&value, &HashMap::new()).html)
        };
        let profile = site.profile.map(|exported| {
            let profile = exported.profile;
            SnapshotProfile {
                display_name: profile.display_name,
                headline: profile.headline,
                bio_html: render(profile.bio_md),
                location: profile.location,
                avatar: exported.avatar,
                experiences: profile
                    .experiences
                    .into_iter()
                    .map(|experience| SnapshotExperience {
                        organization: experience.organization,
                        role: experience.role,
                        description_html: render(experience.description_md),
                        started_on: experience.started_on,
                        ended_on: experience.ended_on,
                    })
                    .collect(),
                skills: profile.skills,
                links: profile.links,
            }
        });
        SiteSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generation,
            generated_at: Utc::now(),
            posts: site.posts,
            categories: site.categories,
            tags: site.tags,
            profile,
            ai_trending: SnapshotTrending::default(),
            media: site.media,
        }
    }
}

/// 写入新对象；产物键按 generation 与尝试序号隔离，已存在说明出现了重复尝试。
async fn put_new(
    storage: &dyn ObjectStorage,
    key: &str,
    bytes: Vec<u8>,
    content_type: &str,
) -> Result<()> {
    match storage.put_if_absent(key, bytes, content_type).await? {
        PutOutcome::Created => Ok(()),
        PutOutcome::AlreadyExists => Err(AppError::Internal(anyhow::anyhow!(
            "artifact object already exists: {key}"
        ))),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests;
