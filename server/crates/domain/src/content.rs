//! 文章、修订、分类标签与发布的领域规则和对外表示。
//!
//! 保存只替换当前编辑修订。显式发布固定一份公开副本，提交该事务后读者和后台
//! 同时看到新的公开修订。撤下只清除公开副本，保留编辑内容。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use silva_shared::error::{AppError, FieldError, FieldErrorCode, Result};
use std::{borrow::Cow, collections::HashSet};
use uuid::Uuid;

/// 文章、分类与标签 slug 的最大字节数。
pub const SLUG_MAX_BYTES: usize = 100;
/// 标题的最大字符数。
pub const TITLE_MAX_CHARS: usize = 200;
/// 摘要的最大字符数。
pub const EXCERPT_MAX_CHARS: usize = 500;
/// Markdown 正文的最大字节数（512 KiB）。
pub const BODY_MAX_BYTES: usize = 512 * 1024;
/// SEO 标题的最大字符数。
pub const SEO_TITLE_MAX_CHARS: usize = 200;
/// SEO 描述的最大字符数。
pub const SEO_DESCRIPTION_MAX_CHARS: usize = 500;
/// 每个修订最多选择的分类数。
pub const MAX_CATEGORIES: usize = 5;
/// 每个修订最多选择的标签数。
pub const MAX_TAGS: usize = 20;
/// 分类、标签名称的最大字符数。
pub const TAXONOMY_NAME_MAX_CHARS: usize = 50;
/// 分类说明的最大字符数。
pub const TAXONOMY_DESCRIPTION_MAX_CHARS: usize = 500;

/// 校验 slug：小写字母、数字和单个连字符组成，不以连字符开头或结尾，1–100 字节。
pub fn validate_slug(
    field: impl Into<Cow<'static, str>>,
    value: &str,
) -> std::result::Result<(), FieldError> {
    let valid = !value.is_empty()
        && value.len() <= SLUG_MAX_BYTES
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    if valid {
        Ok(())
    } else {
        Err(FieldError::new(
            field,
            FieldErrorCode::InvalidFormat,
            "slug must be 1 to 100 bytes of a-z and 0-9 separated by single hyphens",
        ))
    }
}

/// 去首尾空白，空白视为未填写。
fn optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn check_chars(
    errors: &mut Vec<FieldError>,
    field: &'static str,
    value: &str,
    min: usize,
    max: usize,
) {
    let count = value.chars().count();
    if count < min || count > max {
        errors.push(FieldError::new(
            field,
            FieldErrorCode::OutOfRange,
            format!("{field} must contain {min} to {max} characters"),
        ));
    } else if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        errors.push(FieldError::new(
            field,
            FieldErrorCode::InvalidFormat,
            format!("{field} must not contain control characters"),
        ));
    }
}

fn check_ids(errors: &mut Vec<FieldError>, field: &'static str, ids: &[Uuid], max: usize) {
    if ids.len() > max {
        errors.push(FieldError::new(
            field,
            FieldErrorCode::OutOfRange,
            format!("{field} must contain at most {max} items"),
        ));
    }
    let mut seen = HashSet::new();
    for (index, id) in ids.iter().enumerate() {
        if !seen.insert(id) {
            errors.push(FieldError::new(
                format!("{field}[{index}]"),
                FieldErrorCode::Duplicate,
                "id is listed more than once",
            ));
        }
    }
}

/// 一个修订的完整内容。保存修订时整体提交，未变化的字段也要提交。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionContent {
    pub title: String,
    pub excerpt: Option<String>,
    pub markdown_body: String,
    pub category_ids: Vec<Uuid>,
    pub tag_ids: Vec<Uuid>,
    pub cover_media_id: Option<Uuid>,
    pub is_featured: bool,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
}

impl RevisionContent {
    /// 规范化并校验：标题与可选文本去首尾空白，空白的可选文本视为未填写；一次返回全部字段错误。
    pub fn normalize(self) -> Result<Self> {
        let content = Self {
            title: self.title.trim().to_owned(),
            excerpt: optional_text(self.excerpt),
            seo_title: optional_text(self.seo_title),
            seo_description: optional_text(self.seo_description),
            ..self
        };
        let mut errors = Vec::new();
        check_chars(&mut errors, "title", &content.title, 1, TITLE_MAX_CHARS);
        if let Some(excerpt) = &content.excerpt {
            check_chars(&mut errors, "excerpt", excerpt, 1, EXCERPT_MAX_CHARS);
        }
        if let Some(seo_title) = &content.seo_title {
            check_chars(&mut errors, "seo_title", seo_title, 1, SEO_TITLE_MAX_CHARS);
        }
        if let Some(seo_description) = &content.seo_description {
            check_chars(
                &mut errors,
                "seo_description",
                seo_description,
                1,
                SEO_DESCRIPTION_MAX_CHARS,
            );
        }
        if content.markdown_body.len() > BODY_MAX_BYTES {
            errors.push(FieldError::new(
                "markdown_body",
                FieldErrorCode::OutOfRange,
                "markdown_body must contain at most 524288 bytes",
            ));
        } else if content.markdown_body.contains('\0') {
            errors.push(FieldError::new(
                "markdown_body",
                FieldErrorCode::InvalidFormat,
                "markdown_body must not contain NUL characters",
            ));
        }
        check_ids(
            &mut errors,
            "category_ids",
            &content.category_ids,
            MAX_CATEGORIES,
        );
        check_ids(&mut errors, "tag_ids", &content.tag_ids, MAX_TAGS);
        if errors.is_empty() {
            Ok(content)
        } else {
            Err(AppError::Validation(errors))
        }
    }

    /// 与另一份内容是否相同；分类与标签按集合比较。用于自动保存时跳过未变化的内容。
    pub fn same_as(&self, other: &Self) -> bool {
        let set = |ids: &[Uuid]| ids.iter().copied().collect::<HashSet<_>>();
        self.title == other.title
            && self.excerpt == other.excerpt
            && self.markdown_body == other.markdown_body
            && set(&self.category_ids) == set(&other.category_ids)
            && set(&self.tag_ids) == set(&other.tag_ids)
            && self.cover_media_id == other.cover_media_id
            && self.is_featured == other.is_featured
            && self.seo_title == other.seo_title
            && self.seo_description == other.seo_description
    }
}

/// 分类或标签的可编辑字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxonomyInput {
    pub name: String,
    /// 仅分类使用。
    pub description: Option<String>,
}

impl TaxonomyInput {
    pub fn normalize(self) -> Result<Self> {
        let input = Self {
            name: self.name.trim().to_owned(),
            description: optional_text(self.description),
        };
        let mut errors = Vec::new();
        check_chars(&mut errors, "name", &input.name, 1, TAXONOMY_NAME_MAX_CHARS);
        if let Some(description) = &input.description {
            check_chars(
                &mut errors,
                "description",
                description,
                1,
                TAXONOMY_DESCRIPTION_MAX_CHARS,
            );
        }
        if errors.is_empty() {
            Ok(input)
        } else {
            Err(AppError::Validation(errors))
        }
    }
}

/// 目录中的一个标题，`slug` 与渲染 HTML 中标题的 `id` 一致。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub depth: u8,
    pub slug: String,
    pub text: String,
}

/// 用户的公开摘要。
#[derive(Clone, Debug, Serialize)]
pub struct UserRef {
    pub id: Uuid,
    pub display_name: String,
}

/// 分类或标签的引用摘要。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaxonomyRef {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Category {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// 当前编辑版本引用该分类的文章数。
    pub post_count: i64,
    /// 引用该分类的文章修订数（含公开修订）；大于 0 时不能删除。
    pub revision_count: i64,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Tag {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    /// 当前编辑版本引用该标签的文章数。
    pub post_count: i64,
    /// 引用该标签的文章修订数（含公开修订）；大于 0 时不能删除。
    pub revision_count: i64,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

/// 修订的完整内容。
#[derive(Clone, Debug, Serialize)]
pub struct Revision {
    pub id: Uuid,
    pub post_id: Uuid,
    pub revision_no: i32,
    pub title: String,
    pub excerpt: Option<String>,
    pub markdown_body: String,
    /// 保存时渲染并消毒的 HTML；正文图片指向公开媒体地址，后台预览需替换为授权读取地址。
    pub rendered_html: String,
    pub headings: Vec<Heading>,
    pub categories: Vec<TaxonomyRef>,
    pub tags: Vec<TaxonomyRef>,
    pub cover_media_id: Option<Uuid>,
    /// 正文引用的媒体 ID。
    pub media_ids: Vec<Uuid>,
    pub is_featured: bool,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
    pub created_by: Option<UserRef>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
}

/// 修订列表中的摘要。
#[derive(Clone, Debug, Serialize)]
pub struct RevisionSummary {
    pub id: Uuid,
    pub revision_no: i32,
    pub title: String,
    pub created_by: Option<UserRef>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
}

/// 文章的发布状态指针。保存草稿不会覆盖已公开修订。
#[derive(Clone, Debug, Serialize)]
pub struct Publication {
    pub published_revision_id: Option<Uuid>,
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub published_at: Option<DateTime<Utc>>,
    pub publication_version: i64,
    #[serde(serialize_with = "silva_shared::time::serialize_option")]
    pub first_published_at: Option<DateTime<Utc>>,
}

/// 文章列表项。
#[derive(Clone, Debug, Serialize)]
pub struct PostSummary {
    pub series: Option<crate::series::Membership>,
    pub id: Uuid,
    pub slug: String,
    pub slug_locked: bool,
    pub title: String,
    pub categories: Vec<TaxonomyRef>,
    pub author: Option<UserRef>,
    pub current_revision_id: Uuid,
    pub current_revision_no: i32,
    pub published_revision_id: Option<Uuid>,
    pub publication_version: i64,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

/// 文章列表排序方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostSort {
    #[default]
    UpdatedDesc,
    CreatedDesc,
    TitleAsc,
    TitleDesc,
}

impl PostSort {
    pub const ALL: [Self; 4] = [
        Self::UpdatedDesc,
        Self::CreatedDesc,
        Self::TitleAsc,
        Self::TitleDesc,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpdatedDesc => "updated_desc",
            Self::CreatedDesc => "created_desc",
            Self::TitleAsc => "title_asc",
            Self::TitleDesc => "title_desc",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.as_str() == value)
    }
}

/// 单个分类在当前可见范围内的文章数。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PostCategoryCount {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub post_count: i64,
}

/// 当前可见范围内的文章分类统计。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PostCategoryCounts {
    pub total: i64,
    pub uncategorized: i64,
    pub categories: Vec<PostCategoryCount>,
}

/// 文章详情，含当前编辑版本的完整内容。
#[derive(Clone, Debug, Serialize)]
pub struct Post {
    pub series: Option<crate::series::Membership>,
    pub id: Uuid,
    pub slug: String,
    pub slug_locked: bool,
    pub author: Option<UserRef>,
    pub current_revision_id: Uuid,
    pub current_revision: Revision,
    pub publication: Publication,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "silva_shared::time::serialize")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeriesFilter {
    pub series_id: Option<Uuid>,
    pub in_series: Option<bool>,
}
impl SeriesFilter {
    pub fn authorize(self, principal: &crate::identity::Principal) -> Result<()> {
        if self.series_id.is_some() || self.in_series.is_some() {
            principal.require(crate::series::MANAGE)?;
        }
        if self.series_id.is_some() && self.in_series == Some(false) {
            return Err(AppError::invalid(
                "in_series",
                FieldErrorCode::InvalidValue,
                "in_series=false cannot be combined with series_id",
            ));
        }
        Ok(())
    }
}

/// 文章列表筛选。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostFilter {
    pub series: SeriesFilter,
    /// 标题或 slug 关键字，不区分大小写子串匹配。
    pub q: Option<String>,
    pub author_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub tag_id: Option<Uuid>,
    /// true 只返回有公开修订的文章，false 只返回没有公开修订的文章。
    pub published: Option<bool>,
    /// true 只返回未分配分类的文章，false 只返回已分配分类的文章。
    pub uncategorized: Option<bool>,
    pub sort: PostSort,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> RevisionContent {
        RevisionContent {
            title: "  标题 ".into(),
            excerpt: Some("  ".into()),
            markdown_body: "# Hello".into(),
            category_ids: vec![],
            tag_ids: vec![],
            cover_media_id: None,
            is_featured: false,
            seo_title: Some(" SEO ".into()),
            seo_description: None,
        }
    }

    fn fields(error: AppError) -> Vec<(String, FieldErrorCode)> {
        match error {
            AppError::Validation(errors) => errors
                .into_iter()
                .map(|error| (error.field.into_owned(), error.code))
                .collect(),
            other => panic!("期望校验错误，实际为 {other:?}"),
        }
    }

    #[test]
    fn slug_accepts_kebab_case_only() {
        for valid in ["a", "building-with-axum", "v2", "2026-review"] {
            assert!(validate_slug("slug", valid).is_ok(), "{valid}");
        }
        let long = "a".repeat(SLUG_MAX_BYTES + 1);
        for invalid in [
            "",
            "-a",
            "a-",
            "a--b",
            "A",
            "a_b",
            "长林",
            "a b",
            long.as_str(),
        ] {
            assert!(validate_slug("slug", invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn revision_content_trims_and_drops_blank_optional_text() {
        let content = content().normalize().unwrap();
        assert_eq!(content.title, "标题");
        assert_eq!(content.excerpt, None);
        assert_eq!(content.seo_title.as_deref(), Some("SEO"));
    }

    #[test]
    fn revision_content_reports_every_invalid_field() {
        let id = Uuid::now_v7();
        let error = RevisionContent {
            title: " ".into(),
            markdown_body: "x".repeat(BODY_MAX_BYTES + 1),
            category_ids: vec![id, id],
            tag_ids: (0..=MAX_TAGS).map(|_| Uuid::now_v7()).collect(),
            ..content()
        }
        .normalize()
        .unwrap_err();
        assert_eq!(
            fields(error),
            vec![
                ("title".into(), FieldErrorCode::OutOfRange),
                ("markdown_body".into(), FieldErrorCode::OutOfRange),
                ("category_ids[1]".into(), FieldErrorCode::Duplicate),
                ("tag_ids".into(), FieldErrorCode::OutOfRange),
            ]
        );
        assert!(
            RevisionContent {
                title: "a".repeat(TITLE_MAX_CHARS),
                ..content()
            }
            .normalize()
            .is_ok()
        );
        assert!(
            RevisionContent {
                title: "a\u{7}".into(),
                ..content()
            }
            .normalize()
            .is_err()
        );
    }

    #[test]
    fn same_content_compares_taxonomy_as_sets() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        let left = RevisionContent {
            tag_ids: vec![a, b],
            ..content()
        };
        let right = RevisionContent {
            tag_ids: vec![b, a],
            ..content()
        };
        assert!(left.same_as(&right));
        assert!(!left.same_as(&RevisionContent {
            is_featured: true,
            ..right
        }));
    }

    #[test]
    fn taxonomy_input_are_normalized() {
        let input = TaxonomyInput {
            name: " 后端 ".into(),
            description: Some(" ".into()),
        }
        .normalize()
        .unwrap();
        assert_eq!((input.name.as_str(), input.description), ("后端", None));
        assert!(
            TaxonomyInput {
                name: String::new(),
                description: None
            }
            .normalize()
            .is_err()
        );
    }

    #[test]
    fn post_sort_parse() {
        for sort in PostSort::ALL {
            assert_eq!(PostSort::parse(sort.as_str()), Some(sort));
        }
        assert_eq!(PostSort::default(), PostSort::UpdatedDesc);
        assert_eq!(PostSort::parse("oldest"), None);
        assert_eq!(PostSort::parse(""), None);
    }
}
