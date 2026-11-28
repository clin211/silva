//! 分类与标签维护。读取对所有内容角色开放（编辑文章时需要选择），写入需要 `taxonomy.manage`。
//! slug 创建后不能修改；名称与说明修改后，若条目被公开内容引用，在同一事务中推进公开版本。

use crate::{
    content::{POSTS_MANAGE, POSTS_READ_OWN},
    ports::{ContentActor, RequestMeta, TaxonomyItem, TaxonomyKind, TaxonomyRepository},
};
use silva_domain::{
    content::{TaxonomyInput, validate_slug},
    identity::Principal,
    site::SiteVersion,
};
use silva_shared::{
    error::{AppError, FieldErrorCode, Result},
    ids,
};
use std::sync::Arc;
use uuid::Uuid;

pub const TAXONOMY_MANAGE: &str = "taxonomy.manage";

pub struct TaxonomyService {
    repository: Arc<dyn TaxonomyRepository>,
}

impl TaxonomyService {
    pub fn new(repository: Arc<dyn TaxonomyRepository>) -> Self {
        Self { repository }
    }

    #[tracing::instrument(name = "taxonomy.list", skip_all)]
    pub async fn list(
        &self,
        principal: &Principal,
        kind: TaxonomyKind,
    ) -> Result<Vec<TaxonomyItem>> {
        require_read(principal)?;
        self.repository.list(kind).await
    }

    #[tracing::instrument(name = "taxonomy.get", skip_all)]
    pub async fn get(
        &self,
        principal: &Principal,
        kind: TaxonomyKind,
        id: Uuid,
    ) -> Result<TaxonomyItem> {
        require_read(principal)?;
        self.repository
            .find(kind, id)
            .await?
            .ok_or(AppError::NotFound)
    }

    #[tracing::instrument(name = "taxonomy.create", skip_all)]
    pub async fn create(
        &self,
        principal: &Principal,
        kind: TaxonomyKind,
        slug: String,
        input: TaxonomyInput,
        meta: &RequestMeta,
    ) -> Result<TaxonomyItem> {
        principal.require(TAXONOMY_MANAGE)?;
        check_kind(kind, input.description.as_deref())?;
        let slug_error = validate_slug("slug", &slug).err();
        let input = match (input.normalize(), slug_error) {
            (Ok(input), None) => input,
            (Ok(_), Some(error)) => return Err(AppError::Validation(vec![error])),
            (Err(AppError::Validation(mut errors)), Some(error)) => {
                errors.insert(0, error);
                return Err(AppError::Validation(errors));
            }
            (Err(error), _) => return Err(error),
        };
        self.repository
            .create(kind, ids::new_id(), &slug, &input, &actor(principal, meta))
            .await
    }

    /// 部分更新名称与说明：字段为 None 时保持原值，说明传空白字符串时清除。
    /// 返回更新后的条目，以及公开内容发生变化时的新版本。
    #[tracing::instrument(name = "taxonomy.update", skip_all)]
    pub async fn update(
        &self,
        principal: &Principal,
        kind: TaxonomyKind,
        id: Uuid,
        name: Option<String>,
        description: Option<String>,
        meta: &RequestMeta,
    ) -> Result<(TaxonomyItem, Option<SiteVersion>)> {
        principal.require(TAXONOMY_MANAGE)?;
        check_kind(kind, description.as_deref())?;
        self.repository
            .update(kind, id, name, description, &actor(principal, meta))
            .await
    }

    #[tracing::instrument(name = "taxonomy.delete", skip_all)]
    pub async fn delete(
        &self,
        principal: &Principal,
        kind: TaxonomyKind,
        id: Uuid,
        meta: &RequestMeta,
    ) -> Result<()> {
        principal.require(TAXONOMY_MANAGE)?;
        self.repository
            .delete(kind, id, &actor(principal, meta))
            .await
    }
}

/// 标签没有说明字段。
fn check_kind(kind: TaxonomyKind, description: Option<&str>) -> Result<()> {
    if kind == TaxonomyKind::Tag && description.is_some() {
        return Err(AppError::invalid(
            "description",
            FieldErrorCode::UnknownField,
            "tags have no description",
        ));
    }
    Ok(())
}

fn require_read(principal: &Principal) -> Result<()> {
    [POSTS_READ_OWN, POSTS_MANAGE, TAXONOMY_MANAGE]
        .iter()
        .any(|permission| principal.require(permission).is_ok())
        .then_some(())
        .ok_or(AppError::Forbidden)
}

fn actor<'a>(principal: &Principal, meta: &'a RequestMeta) -> ContentActor<'a> {
    ContentActor {
        user_id: principal.user.id,
        meta,
    }
}
