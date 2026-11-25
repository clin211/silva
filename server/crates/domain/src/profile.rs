//! 单例个人资料的编辑表示。子项按提交顺序展示，保存时整体替换。

use crate::site::{SnapshotLink, SnapshotSkill};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use silva_shared::{
    error::{AppError, FieldError, FieldErrorCode, Result},
    ids::EntityId,
};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub display_name: String,
    pub headline: Option<String>,
    pub bio_md: Option<String>,
    pub location: Option<String>,
    pub avatar_media_id: Option<EntityId>,
    pub experiences: Vec<Experience>,
    pub skills: Vec<SnapshotSkill>,
    pub links: Vec<SnapshotLink>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experience {
    pub organization: String,
    pub role: String,
    pub description_md: Option<String>,
    pub started_on: Option<NaiveDate>,
    pub ended_on: Option<NaiveDate>,
}

fn text_field(errors: &mut Vec<FieldError>, field: String, value: &str, required: bool) {
    if required && value.trim().is_empty() {
        errors.push(FieldError::new(
            field,
            FieldErrorCode::Required,
            "must not be blank",
        ));
    } else if value.contains('\0') {
        errors.push(FieldError::new(
            field,
            FieldErrorCode::InvalidFormat,
            "must not contain NUL",
        ));
    }
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        let mut errors = Vec::new();
        text_field(&mut errors, "display_name".into(), &self.display_name, true);
        for (field, value) in [
            ("headline", &self.headline),
            ("bio_md", &self.bio_md),
            ("location", &self.location),
        ] {
            if let Some(value) = value {
                text_field(&mut errors, field.into(), value, false);
            }
        }
        for (i, entry) in self.experiences.iter().enumerate() {
            let field = |name| format!("experiences[{i}].{name}");
            text_field(
                &mut errors,
                field("organization"),
                &entry.organization,
                true,
            );
            text_field(&mut errors, field("role"), &entry.role, true);
            if let Some(value) = &entry.description_md {
                text_field(&mut errors, field("description_md"), value, false);
            }
            for (name, date) in [
                ("started_on", entry.started_on),
                ("ended_on", entry.ended_on),
            ] {
                if date.is_some_and(|day| !(1..=9999).contains(&day.year())) {
                    errors.push(FieldError::new(
                        field(name),
                        FieldErrorCode::InvalidFormat,
                        "expected a date from year 0001 to 9999",
                    ));
                }
            }
            if let (Some(start), Some(end)) = (entry.started_on, entry.ended_on)
                && end < start
            {
                errors.push(FieldError::new(
                    field("ended_on"),
                    FieldErrorCode::InvalidValue,
                    "must not precede started_on",
                ));
            }
        }
        for (i, entry) in self.skills.iter().enumerate() {
            text_field(&mut errors, format!("skills[{i}].name"), &entry.name, true);
            for (field, value) in [
                ("category", &entry.category),
                ("proficiency", &entry.proficiency),
            ] {
                if let Some(value) = value {
                    text_field(&mut errors, format!("skills[{i}].{field}"), value, false);
                }
            }
        }
        for (i, entry) in self.links.iter().enumerate() {
            text_field(
                &mut errors,
                format!("links[{i}].platform"),
                &entry.platform,
                true,
            );
            if let Some(value) = &entry.label {
                text_field(&mut errors, format!("links[{i}].label"), value, false);
            }
            if fluent_uri::Uri::parse(entry.url.as_str()).is_err() {
                errors.push(FieldError::new(
                    format!("links[{i}].url"),
                    FieldErrorCode::InvalidFormat,
                    "expected an absolute URI",
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation(errors))
        }
    }
}
