//! 公开内容版本与供 SSR 使用的数据。结构见 `docs/contracts/site-snapshot.md`。

use crate::content::Heading;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 公开快照结构版本。
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// 公开内容版本号。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SiteVersion {
    pub generation: i64,
}

/// 内容上线或撤下事务已提交的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationResult {
    pub generation: i64,
    pub publication_version: i64,
}

/// 供公开接口与页面共用的已发布数据。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteSnapshot {
    pub schema_version: u32,
    pub generation: i64,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub generated_at: DateTime<Utc>,
    #[serde(default)]
    pub series: Vec<SnapshotSeries>,
    pub posts: Vec<SnapshotPost>,
    pub categories: Vec<SnapshotCategory>,
    pub tags: Vec<SnapshotTag>,
    pub profile: Option<SnapshotProfile>,
    pub ai_trending: SnapshotTrending,
    pub media: Vec<SnapshotMedia>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotSeries {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub category: Option<SnapshotCategory>,
    pub completion_status: crate::series::CompletionStatus,
    pub featured: bool,
    pub cover: Option<SnapshotImage>,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub published_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
    pub items: Vec<SnapshotSeriesItem>,
    pub sections: Vec<SnapshotSection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotSection {
    pub title: String,
    pub description: Option<String>,
    pub items: Vec<SnapshotSeriesItem>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotSeriesItem {
    pub post_slug: String,
    pub display_title: Option<String>,
}

impl SiteSnapshot {
    pub fn validate_series(&self) -> Result<(), String> {
        use std::collections::HashSet;
        let posts: HashSet<_> = self.posts.iter().map(|p| p.slug.as_str()).collect();
        let media: HashSet<_> = self.media.iter().map(|m| m.id).collect();
        let mut ids = HashSet::new();
        let mut slugs = HashSet::new();
        let mut members = HashSet::new();
        for s in &self.series {
            if !ids.insert(s.id)
                || !slugs.insert(&s.slug)
                || (s.items.is_empty() && s.sections.is_empty())
                || s.sections.iter().any(|group| group.items.is_empty())
                || s.cover
                    .as_ref()
                    .is_some_and(|cover| !media.contains(&cover.media_id))
                || s.items
                    .iter()
                    .chain(s.sections.iter().flat_map(|group| &group.items))
                    .any(|item| {
                        !posts.contains(item.post_slug.as_str()) || !members.insert(&item.post_slug)
                    })
            {
                return Err(format!("invalid series outline or reference: {}", s.id));
            }
        }
        Ok(())
    }
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
#[serde(deny_unknown_fields)]
pub struct SnapshotSkill {
    pub name: String,
    pub category: Option<String>,
    pub proficiency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_with_beijing_offset() {
        let snapshot = SiteSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            generation: 3,
            generated_at: "2026-11-24T08:00:00Z".parse().unwrap(),
            series: vec![],
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
        assert_eq!(json["series"], serde_json::json!([]));
        let mut legacy = json.clone();
        legacy.as_object_mut().unwrap().remove("series");
        assert_eq!(
            serde_json::from_value::<SiteSnapshot>(legacy.clone()).unwrap(),
            snapshot
        );
        legacy["series"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<SiteSnapshot>(legacy).is_err());
        let parsed: SiteSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, snapshot);
    }
}
