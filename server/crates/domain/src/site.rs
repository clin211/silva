//! 站点发布：发布任务状态、构建快照与产物清单。快照结构见 `docs/contracts/site-snapshot.md`。

use crate::content::{Heading, UserRef};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 构建器支持的快照结构版本。
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// 发布任务状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishJobStatus {
    /// 等待 Worker 领取。
    Queued,
    /// Worker 正在导出、构建或上传。
    Building,
    /// 当前线上版本。
    Active,
    /// 构建失败且已用尽自动重试；仍为最新目标时可由管理员重试。
    Failed,
    /// 上线前已有更新的目标，不再构建。
    Superseded,
    /// 曾经上线，已被更新的版本取代。
    Retired,
}

impl PublishJobStatus {
    pub const ALL: [Self; 6] = [
        Self::Queued,
        Self::Building,
        Self::Active,
        Self::Failed,
        Self::Superseded,
        Self::Retired,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Building => "building",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
            Self::Retired => "retired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

/// 发布任务。
#[derive(Clone, Debug, Serialize)]
pub struct PublishJob {
    pub id: Uuid,
    pub generation: i64,
    pub status: PublishJobStatus,
    /// 触发来源，如 `post.publish`、`post.unpublish`、`taxonomy.update`、`manual`、`bootstrap`。
    pub trigger_type: String,
    pub requested_by: Option<UserRef>,
    /// 已开始的构建次数（含自动重试）。
    pub attempts: i32,
    /// 最近一次失败的原因；成功或尚未失败时为 None。
    pub error_message: Option<String>,
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
}

/// 发布任务列表筛选。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublishJobFilter {
    pub status: Option<PublishJobStatus>,
    pub generation: Option<i64>,
    /// 只返回该账户触发的任务；只有 `jobs.read_own` 时由用例强制设为当前账户。
    pub requested_by: Option<Uuid>,
}

/// 登记站点目标的结果：新的发布任务及其 generation。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SiteTarget {
    pub publish_job_id: Uuid,
    pub desired_generation: i64,
}

/// 文章上线或撤下被接受后的结果；不代表已经上线。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationAccepted {
    pub publish_job_id: Uuid,
    pub desired_generation: i64,
    pub publication_version: i64,
}

/// 站点构建快照，只含公开投影。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteSnapshot {
    pub schema_version: u32,
    pub generation: i64,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub generated_at: DateTime<Utc>,
    pub posts: Vec<SnapshotPost>,
    pub categories: Vec<SnapshotCategory>,
    pub tags: Vec<SnapshotTag>,
    pub profile: Option<SnapshotProfile>,
    pub ai_trending: SnapshotTrending,
    pub media: Vec<SnapshotMedia>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPost {
    pub id: Uuid,
    pub revision_id: Uuid,
    pub slug: String,
    pub title: String,
    pub excerpt: Option<String>,
    pub html: String,
    pub headings: Vec<Heading>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub featured: bool,
    pub cover: Option<SnapshotImage>,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub published_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotImage {
    pub media_id: Uuid,
    pub mime_type: String,
    pub width: i32,
    pub height: i32,
    pub alt: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotCategory {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotTag {
    pub slug: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotProfile {
    pub display_name: String,
    pub headline: Option<String>,
    pub bio_html: Option<String>,
    pub location: Option<String>,
    pub avatar: Option<SnapshotImage>,
    pub experiences: Vec<SnapshotExperience>,
    pub skills: Vec<SnapshotSkill>,
    pub links: Vec<SnapshotLink>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotExperience {
    pub organization: String,
    pub role: String,
    pub description_html: Option<String>,
    pub started_on: Option<NaiveDate>,
    pub ended_on: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotSkill {
    pub name: String,
    pub category: Option<String>,
    pub proficiency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotLink {
    pub platform: String,
    pub label: Option<String>,
    pub url: String,
}

/// AI 榜单；采集实现前固定为无数据。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapshotTrending {
    pub data_date: Option<NaiveDate>,
    pub repositories: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotMedia {
    pub id: Uuid,
    pub mime_type: String,
    pub width: i32,
    pub height: i32,
}

/// 构建产物清单，与产物一同上传。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub generation: i64,
    pub attempt: i32,
    pub snapshot_sha256: String,
    pub files: Vec<ArtifactFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactFile {
    /// 相对产物根目录的路径，使用 `/` 分隔，不以 `/` 开头。
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub content_type: String,
}

/// 一次构建的对象存储位置。
pub fn attempt_prefix(generation: i64, attempt: i32) -> String {
    format!("site/{generation}/{attempt}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_status_round_trips() {
        for status in PublishJobStatus::ALL {
            assert_eq!(PublishJobStatus::parse(status.as_str()), Some(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::json!(status.as_str())
            );
        }
    }

    #[test]
    fn snapshot_round_trips_with_beijing_offset() {
        let snapshot = SiteSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generation: 3,
            generated_at: "2026-11-24T08:00:00Z".parse().unwrap(),
            posts: vec![],
            categories: vec![],
            tags: vec![],
            profile: None,
            ai_trending: SnapshotTrending::default(),
            media: vec![],
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["generated_at"], "2026-11-24T16:00:00+08:00");
        assert_eq!(
            json["ai_trending"],
            serde_json::json!({"data_date": null, "repositories": []})
        );
        let parsed: SiteSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, snapshot);
        assert_eq!(attempt_prefix(3, 2), "site/3/2/");
    }
}
