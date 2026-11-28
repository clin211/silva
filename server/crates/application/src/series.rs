//! 系列管理用例与事务仓储契约。
use crate::ports::RequestMeta;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use silva_domain::{
    identity::Principal,
    series::{CompletionStatus, ItemInput, MANAGE, Metadata, MetadataPatch, Outline},
    site::{PublicationResult, SiteVersion},
};
use silva_shared::{
    error::{AppError, FieldErrorCode, Result},
    pagination::{Page, PageRequest},
};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Summary {
    pub id: Uuid,
    #[serde(flatten)]
    pub metadata: Metadata,
    pub slug_locked: bool,
    pub edit_version: i64,
    pub publication_version: i64,
    pub item_count: i64,
    pub published_post_count: i64,
    pub is_live: bool,
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub first_published_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Item {
    pub id: Uuid,
    pub post_id: Uuid,
    pub section_id: Option<Uuid>,
    pub position: i32,
    pub display_title: Option<String>,
    pub title: String,
    pub slug: String,
    pub current_revision_id: Uuid,
    pub published_revision_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Section {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub position: i32,
    pub items: Vec<Item>,
}

#[derive(Debug, Serialize)]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub sections: Vec<Section>,
    pub items: Vec<Item>,
}
impl Detail {
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .chain(self.sections.iter().flat_map(|s| &s.items))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    SortOrderAsc,
    UpdatedDesc,
    CreatedDesc,
}

#[derive(Debug, Default)]
pub struct Filter {
    pub q: Option<String>,
    pub category_id: Option<Uuid>,
    pub published: Option<bool>,
    pub completion_status: Option<CompletionStatus>,
    pub sort: Sort,
}

pub enum Edit {
    Metadata(MetadataPatch),
    Outline(Outline),
    Append {
        version: i64,
        section: Option<Uuid>,
        item: ItemInput,
    },
    Remove {
        version: i64,
        post: Uuid,
    },
}
impl Edit {
    pub fn version(&self) -> i64 {
        match self {
            Self::Metadata(p) => p.expected_edit_version,
            Self::Outline(o) => o.expected_edit_version,
            Self::Append { version, .. } | Self::Remove { version, .. } => *version,
        }
    }
}

#[derive(Serialize)]
pub struct Edited {
    pub series: Detail,
    pub publication: Option<SiteVersion>,
}

pub struct Actor<'a> {
    pub principal: &'a Principal,
    pub meta: &'a RequestMeta,
}

#[async_trait]
pub trait SeriesRepository: Send + Sync {
    async fn list(&self, page: &PageRequest, filter: &Filter) -> Result<Page<Summary>>;
    async fn detail(&self, id: Uuid) -> Result<Detail>;
    async fn create(&self, input: Metadata, actor: Actor<'_>) -> Result<Detail>;
    async fn edit(&self, id: Uuid, edit: Edit, actor: Actor<'_>) -> Result<Edited>;
    async fn publish(
        &self,
        id: Uuid,
        version: i64,
        published: bool,
        actor: Actor<'_>,
    ) -> Result<PublicationResult>;
    async fn delete(
        &self,
        id: Uuid,
        version: i64,
        publication_version: i64,
        actor: Actor<'_>,
    ) -> Result<()>;
}

fn version(field: &'static str, value: i64) -> Result<()> {
    if value < 0 {
        return Err(AppError::invalid(
            field,
            FieldErrorCode::OutOfRange,
            "version must be non-negative",
        ));
    }
    Ok(())
}

pub struct SeriesService {
    repository: Arc<dyn SeriesRepository>,
}
impl SeriesService {
    pub fn new(repository: Arc<dyn SeriesRepository>) -> Self {
        Self { repository }
    }

    pub async fn list(
        &self,
        principal: &Principal,
        page: &PageRequest,
        filter: &Filter,
    ) -> Result<Page<Summary>> {
        principal.require(MANAGE)?;
        page.validate()?;
        self.repository.list(page, filter).await
    }
    pub async fn detail(&self, principal: &Principal, id: Uuid) -> Result<Detail> {
        principal.require(MANAGE)?;
        self.repository.detail(id).await
    }
    pub async fn create(&self, mut input: Metadata, actor: Actor<'_>) -> Result<Detail> {
        actor.principal.require(MANAGE)?;
        input.normalize()?;
        self.repository.create(input, actor).await
    }
    pub async fn edit(&self, id: Uuid, mut edit: Edit, actor: Actor<'_>) -> Result<Edited> {
        actor.principal.require(MANAGE)?;
        version("expected_edit_version", edit.version())?;
        match &mut edit {
            Edit::Outline(outline) => outline.normalize()?,
            Edit::Append { item, .. } => item.normalize("display_title")?,
            Edit::Metadata(_) | Edit::Remove { .. } => {}
        }
        self.repository.edit(id, edit, actor).await
    }
    pub async fn publish(
        &self,
        id: Uuid,
        expected: i64,
        published: bool,
        actor: Actor<'_>,
    ) -> Result<PublicationResult> {
        actor.principal.require(MANAGE)?;
        actor.principal.require("posts.publish")?;
        version("expected_publication_version", expected)?;
        self.repository
            .publish(id, expected, published, actor)
            .await
    }
    pub async fn delete(
        &self,
        id: Uuid,
        edit: i64,
        publication: i64,
        actor: Actor<'_>,
    ) -> Result<()> {
        actor.principal.require(MANAGE)?;
        version("expected_edit_version", edit)?;
        version("expected_publication_version", publication)?;
        self.repository.delete(id, edit, publication, actor).await
    }
}
