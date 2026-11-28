//! 系列元数据、目录输入与编排规则。
use crate::content::{SEO_DESCRIPTION_MAX_CHARS, SEO_TITLE_MAX_CHARS, validate_slug};
use serde::{Deserialize, Deserializer, Serialize};
use silva_shared::{
    error::{AppError, FieldErrorCode, Result},
    ids::EntityId,
};
use std::collections::HashSet;
use uuid::Uuid;

pub const MANAGE: &str = "series.manage";
pub const MAX_SECTIONS: usize = 50;
pub const MAX_ITEMS: usize = 200;

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    #[default]
    Ongoing,
    Completed,
}

impl CompletionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ongoing => "ongoing",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub cover_media_id: Option<EntityId>,
    pub category_id: Option<EntityId>,
    #[serde(default)]
    pub completion_status: CompletionStatus,
    #[serde(default)]
    pub is_featured: bool,
    #[serde(default)]
    pub sort_order: i32,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
}

fn title(value: &mut String, field: &str) -> Result<()> {
    *value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > 100 || value.chars().any(char::is_control) {
        return Err(AppError::invalid(
            field.to_owned(),
            FieldErrorCode::InvalidValue,
            "expected 1 to 100 characters without control characters",
        ));
    }
    Ok(())
}

fn optional(value: &mut Option<String>, field: &str, max: usize) -> Result<()> {
    *value = value
        .take()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    if value.as_ref().is_some_and(|s| s.contains('\0')) {
        return Err(AppError::invalid(
            field.to_owned(),
            FieldErrorCode::InvalidFormat,
            "text must not contain NUL characters",
        ));
    }
    if value.as_ref().is_some_and(|s| s.chars().count() > max) {
        return Err(AppError::invalid(
            field.to_owned(),
            FieldErrorCode::OutOfRange,
            format!("maximum length is {max} characters"),
        ));
    }
    Ok(())
}

impl Metadata {
    pub fn normalize(&mut self) -> Result<()> {
        validate_slug("slug", &self.slug).map_err(|e| AppError::Validation(vec![e]))?;
        title(&mut self.title, "title")?;
        optional(&mut self.description, "description", 1000)?;
        optional(&mut self.seo_title, "seo_title", SEO_TITLE_MAX_CHARS)?;
        optional(
            &mut self.seo_description,
            "seo_description",
            SEO_DESCRIPTION_MAX_CHARS,
        )
    }
}

// 缺省由 default 表示；出现时按字段原有类型解析，可空字段保留显式 null。
fn supplied<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataPatch {
    pub expected_edit_version: i64,
    #[serde(default, deserialize_with = "supplied")]
    slug: Option<String>,
    #[serde(default, deserialize_with = "supplied")]
    title: Option<String>,
    #[serde(default, deserialize_with = "supplied")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "supplied")]
    cover_media_id: Option<Option<EntityId>>,
    #[serde(default, deserialize_with = "supplied")]
    category_id: Option<Option<EntityId>>,
    #[serde(default, deserialize_with = "supplied")]
    completion_status: Option<CompletionStatus>,
    #[serde(default, deserialize_with = "supplied")]
    is_featured: Option<bool>,
    #[serde(default, deserialize_with = "supplied")]
    sort_order: Option<i32>,
    #[serde(default, deserialize_with = "supplied")]
    seo_title: Option<Option<String>>,
    #[serde(default, deserialize_with = "supplied")]
    seo_description: Option<Option<String>>,
}

impl MetadataPatch {
    pub fn apply(self, current: &mut Metadata) {
        if let Some(value) = self.slug {
            current.slug = value;
        }
        if let Some(value) = self.title {
            current.title = value;
        }
        if let Some(value) = self.description {
            current.description = value;
        }
        if let Some(value) = self.cover_media_id {
            current.cover_media_id = value;
        }
        if let Some(value) = self.category_id {
            current.category_id = value;
        }
        if let Some(value) = self.completion_status {
            current.completion_status = value;
        }
        if let Some(value) = self.is_featured {
            current.is_featured = value;
        }
        if let Some(value) = self.sort_order {
            current.sort_order = value;
        }
        if let Some(value) = self.seo_title {
            current.seo_title = value;
        }
        if let Some(value) = self.seo_description {
            current.seo_description = value;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ItemInput {
    pub post_id: EntityId,
    pub display_title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionInput {
    pub id: Option<EntityId>,
    pub title: String,
    pub description: Option<String>,
    pub items: Vec<ItemInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outline {
    pub expected_edit_version: i64,
    pub sections: Vec<SectionInput>,
    pub items: Vec<ItemInput>,
}

impl ItemInput {
    pub fn normalize(&mut self, field: &str) -> Result<()> {
        optional(&mut self.display_title, field, 200)
    }
}

impl Outline {
    pub fn normalize(&mut self) -> Result<()> {
        if self.sections.len() > MAX_SECTIONS {
            return Err(AppError::invalid(
                "sections",
                FieldErrorCode::OutOfRange,
                "at most 50 sections",
            ));
        }
        if self.items.len() + self.sections.iter().map(|s| s.items.len()).sum::<usize>() > MAX_ITEMS
        {
            return Err(AppError::invalid(
                "items",
                FieldErrorCode::OutOfRange,
                "at most 200 posts",
            ));
        }
        let mut posts = HashSet::new();
        let mut sections = HashSet::new();
        let mut group = |items: &mut [ItemInput], path: &str| -> Result<()> {
            for (i, item) in items.iter_mut().enumerate() {
                if !posts.insert(item.post_id.as_uuid()) {
                    return Err(AppError::invalid(
                        format!("{path}[{i}].post_id"),
                        FieldErrorCode::Duplicate,
                        "post appears more than once",
                    ));
                }
                item.normalize(&format!("{path}[{i}].display_title"))?;
            }
            Ok(())
        };
        group(&mut self.items, "items")?;
        for (i, section) in self.sections.iter_mut().enumerate() {
            let path = format!("sections[{i}]");
            if section.id.is_some_and(|id| !sections.insert(id.as_uuid())) {
                return Err(AppError::invalid(
                    format!("{path}.id"),
                    FieldErrorCode::Duplicate,
                    "section appears more than once",
                ));
            }
            title(&mut section.title, &format!("{path}.title"))?;
            optional(
                &mut section.description,
                &format!("{path}.description"),
                500,
            )?;
            group(&mut section.items, &format!("{path}.items"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membership {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub section_title: Option<String>,
    pub position: i32,
}
