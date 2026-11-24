//! 文章、修订、审核与发布用例。
//!
//! 授权规则（产品文档 §3）：
//! - `posts.manage`（Editor、Admin）可读写全部文章；只有 `posts.*_own` 时只能访问本人文章，
//!   他人文章按不存在处理（404）。
//! - 审核决定需要 `posts.review`，上线与撤下需要 `posts.publish`。Editor/Admin 可以审核自己写的
//!   修订，但必须显式提交并批准，不能跳过审批。
//! - 修订引用的媒体必须已就绪，且调用者有权引用：持有 `media.manage`、是媒体上传者，
//!   或该媒体已被当前编辑版本引用。

use crate::ports::{
    ContentActor, ContentRepository, ImageSize, MarkdownRenderer, NewPost, NewRevision, PostAccess,
    RequestMeta, SavedRevision,
};
use silva_domain::{
    content::{
        Post, PostFilter, PostSummary, Review, ReviewDecision, Revision, RevisionContent,
        RevisionSummary, normalize_review_comment, validate_slug,
    },
    identity::Principal,
    media::MediaStatus,
    site::PublicationAccepted,
};
use silva_shared::{
    error::{AppError, Conflict, FieldError, FieldErrorCode, Result},
    ids,
    pagination::{Page, PageRequest},
};
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

pub const POSTS_CREATE: &str = "posts.create";
pub const POSTS_READ_OWN: &str = "posts.read_own";
pub const POSTS_EDIT_OWN: &str = "posts.edit_own";
pub const POSTS_SUBMIT_OWN: &str = "posts.submit_own";
pub const POSTS_MANAGE: &str = "posts.manage";
pub const POSTS_REVIEW: &str = "posts.review";
pub const POSTS_PUBLISH: &str = "posts.publish";
const MEDIA_MANAGE: &str = "media.manage";

pub struct ContentService {
    repository: Arc<dyn ContentRepository>,
    renderer: Arc<dyn MarkdownRenderer>,
}

/// 保存修订的结果，API 层据此返回 201 或 200。
pub enum RevisionResult {
    Created(Revision),
    Unchanged(Revision),
}

impl ContentService {
    pub fn new(
        repository: Arc<dyn ContentRepository>,
        renderer: Arc<dyn MarkdownRenderer>,
    ) -> Self {
        Self {
            repository,
            renderer,
        }
    }

    #[tracing::instrument(name = "content.create_post", skip_all)]
    pub async fn create_post(
        &self,
        principal: &Principal,
        slug: String,
        content: RevisionContent,
        meta: &RequestMeta,
    ) -> Result<Post> {
        principal.require(POSTS_CREATE)?;
        let slug_error = validate_slug("slug", &slug).err();
        let revision = match (self.prepare(principal, content, &[]).await, slug_error) {
            (Ok(revision), None) => revision,
            (Ok(_), Some(error)) => return Err(AppError::Validation(vec![error])),
            (Err(AppError::Validation(mut errors)), Some(error)) => {
                errors.insert(0, error);
                return Err(AppError::Validation(errors));
            }
            (Err(error), _) => return Err(error),
        };
        self.repository
            .create_post(
                NewPost {
                    id: ids::new_id(),
                    slug,
                    author_id: principal.user.id,
                    revision,
                },
                &actor(principal, meta),
            )
            .await
    }

    #[tracing::instrument(name = "content.list_posts", skip_all)]
    pub async fn list_posts(
        &self,
        principal: &Principal,
        page: &PageRequest,
        filter: PostFilter,
    ) -> Result<Page<PostSummary>> {
        page.validate()?;
        let filter = if principal.require(POSTS_MANAGE).is_ok() {
            filter
        } else {
            principal.require(POSTS_READ_OWN)?;
            PostFilter {
                author_id: Some(principal.user.id),
                ..filter
            }
        };
        self.repository.list_posts(page, &filter).await
    }

    #[tracing::instrument(name = "content.post", skip_all)]
    pub async fn post(&self, principal: &Principal, id: Uuid) -> Result<Post> {
        self.access(principal, id, POSTS_READ_OWN).await?;
        self.repository.post(id).await?.ok_or(AppError::NotFound)
    }

    /// 保存修订（含自动保存）。内容与当前编辑版本相同时不创建新修订。
    #[tracing::instrument(name = "content.create_revision", skip_all)]
    pub async fn create_revision(
        &self,
        principal: &Principal,
        post_id: Uuid,
        expected_revision_id: Uuid,
        content: RevisionContent,
        meta: &RequestMeta,
    ) -> Result<RevisionResult> {
        let post = self.access(principal, post_id, POSTS_EDIT_OWN).await?;
        let allowed = self
            .repository
            .revision_media(post.current_revision_id)
            .await?;
        let revision = self.prepare(principal, content, &allowed).await?;
        match self
            .repository
            .create_revision(
                post_id,
                expected_revision_id,
                revision,
                &actor(principal, meta),
            )
            .await?
        {
            SavedRevision::Created(revision) => Ok(RevisionResult::Created(revision)),
            SavedRevision::Unchanged(revision) => Ok(RevisionResult::Unchanged(revision)),
        }
    }

    /// 修改 slug；首次被接受发布后不能再修改。
    #[tracing::instrument(name = "content.update_slug", skip_all)]
    pub async fn update_slug(
        &self,
        principal: &Principal,
        post_id: Uuid,
        slug: String,
        meta: &RequestMeta,
    ) -> Result<Post> {
        self.access(principal, post_id, POSTS_EDIT_OWN).await?;
        validate_slug("slug", &slug).map_err(|error| AppError::Validation(vec![error]))?;
        self.repository
            .update_slug(post_id, &slug, &actor(principal, meta))
            .await
    }

    #[tracing::instrument(name = "content.revisions", skip_all)]
    pub async fn revisions(
        &self,
        principal: &Principal,
        post_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<RevisionSummary>> {
        page.validate()?;
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        self.repository.revisions(post_id, page).await
    }

    #[tracing::instrument(name = "content.revision", skip_all)]
    pub async fn revision(
        &self,
        principal: &Principal,
        post_id: Uuid,
        revision_id: Uuid,
    ) -> Result<Revision> {
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        self.repository
            .revision(post_id, revision_id)
            .await?
            .ok_or(AppError::NotFound)
    }

    /// 提交指定修订审核。
    #[tracing::instrument(name = "content.create_review", skip_all)]
    pub async fn create_review(
        &self,
        principal: &Principal,
        post_id: Uuid,
        revision_id: Uuid,
        meta: &RequestMeta,
    ) -> Result<Review> {
        self.access(principal, post_id, POSTS_SUBMIT_OWN).await?;
        if self
            .repository
            .revision(post_id, revision_id)
            .await?
            .is_none()
        {
            return Err(unknown_revision());
        }
        self.repository
            .create_review(post_id, revision_id, &actor(principal, meta))
            .await
    }

    /// 批准或拒绝待审核记录。
    #[tracing::instrument(name = "content.decide_review", skip_all)]
    pub async fn decide_review(
        &self,
        principal: &Principal,
        post_id: Uuid,
        review_id: Uuid,
        decision: &str,
        comment: Option<String>,
        meta: &RequestMeta,
    ) -> Result<Review> {
        principal.require(POSTS_REVIEW)?;
        let decision = ReviewDecision::parse(decision).ok_or_else(|| {
            AppError::invalid(
                "decision",
                FieldErrorCode::InvalidValue,
                "decision must be approved or rejected",
            )
        })?;
        let comment = normalize_review_comment(comment)?;
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        self.repository
            .decide_review(
                post_id,
                review_id,
                decision,
                comment,
                &actor(principal, meta),
            )
            .await
    }

    #[tracing::instrument(name = "content.reviews", skip_all)]
    pub async fn reviews(
        &self,
        principal: &Principal,
        post_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Review>> {
        page.validate()?;
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        self.repository.reviews(post_id, page).await
    }

    #[tracing::instrument(name = "content.review", skip_all)]
    pub async fn review(
        &self,
        principal: &Principal,
        post_id: Uuid,
        review_id: Uuid,
    ) -> Result<Review> {
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        self.repository
            .review(post_id, review_id)
            .await?
            .ok_or(AppError::NotFound)
    }

    /// 接受一个已批准的修订作为期望公开版本，并登记站点目标；不代表已经上线。
    #[tracing::instrument(name = "content.publish", skip_all)]
    pub async fn publish(
        &self,
        principal: &Principal,
        post_id: Uuid,
        revision_id: Uuid,
        expected_version: i64,
        meta: &RequestMeta,
    ) -> Result<PublicationAccepted> {
        principal.require(POSTS_PUBLISH)?;
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        match self
            .repository
            .publish(
                post_id,
                revision_id,
                expected_version,
                &actor(principal, meta),
            )
            .await
        {
            // 文章已确认存在，NotFound 只可能来自修订。
            Err(AppError::NotFound) => Err(unknown_revision()),
            other => other,
        }
    }

    /// 清空期望公开版本并登记站点目标；撤下在新的站点版本上线后生效。
    #[tracing::instrument(name = "content.unpublish", skip_all)]
    pub async fn unpublish(
        &self,
        principal: &Principal,
        post_id: Uuid,
        expected_version: i64,
        meta: &RequestMeta,
    ) -> Result<PublicationAccepted> {
        principal.require(POSTS_PUBLISH)?;
        self.access(principal, post_id, POSTS_READ_OWN).await?;
        self.repository
            .unpublish(post_id, expected_version, &actor(principal, meta))
            .await
    }

    /// 检查调用者能否以 `own_permission` 访问文章：`posts.manage` 可访问全部文章；
    /// 否则必须是作者并持有该权限。看不到的文章按不存在处理。
    async fn access(
        &self,
        principal: &Principal,
        post_id: Uuid,
        own_permission: &str,
    ) -> Result<PostAccess> {
        let manage = principal.require(POSTS_MANAGE).is_ok();
        if !manage {
            principal.require(POSTS_READ_OWN)?;
        }
        let post = self
            .repository
            .post_access(post_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if manage {
            return Ok(post);
        }
        if post.author_id != Some(principal.user.id) {
            return Err(AppError::NotFound);
        }
        principal.require(own_permission)?;
        Ok(post)
    }

    /// 校验修订内容、分类标签与媒体引用，并渲染正文。
    async fn prepare(
        &self,
        principal: &Principal,
        content: RevisionContent,
        allowed_media: &[Uuid],
    ) -> Result<NewRevision> {
        let content = content.normalize()?;
        let body_media = self
            .renderer
            .media_references("markdown_body", &content.markdown_body)?;
        let mut errors = Vec::new();
        let (missing_categories, missing_tags) = self
            .repository
            .missing_taxonomy(&content.category_ids, &content.tag_ids)
            .await?;
        for (field, ids, missing) in [
            ("category_ids", &content.category_ids, &missing_categories),
            ("tag_ids", &content.tag_ids, &missing_tags),
        ] {
            for (index, id) in ids.iter().enumerate() {
                if missing.contains(id) {
                    errors.push(FieldError::new(
                        format!("{field}[{index}]"),
                        FieldErrorCode::InvalidValue,
                        "the referenced item does not exist",
                    ));
                }
            }
        }
        let mut requested = body_media.clone();
        if let Some(cover) = content.cover_media_id
            && !requested.contains(&cover)
        {
            requested.push(cover);
        }
        let found: HashMap<Uuid, _> = self
            .repository
            .media_references(&requested)
            .await?
            .into_iter()
            .map(|media| (media.id, media))
            .collect();
        let manage_media = principal.require(MEDIA_MANAGE).is_ok();
        let referable = |id: &Uuid| {
            found.get(id).is_some_and(|media| {
                manage_media
                    || media.uploaded_by == Some(principal.user.id)
                    || allowed_media.contains(id)
            })
        };
        if let Some(cover) = content.cover_media_id
            && !referable(&cover)
        {
            errors.push(FieldError::new(
                "cover_media_id",
                FieldErrorCode::InvalidValue,
                "the media does not exist or cannot be referenced",
            ));
        }
        if !body_media.iter().all(referable) {
            errors.push(FieldError::new(
                "markdown_body",
                FieldErrorCode::InvalidValue,
                "an image references media that does not exist or cannot be referenced",
            ));
        }
        if !errors.is_empty() {
            return Err(AppError::Validation(errors));
        }
        if requested
            .iter()
            .any(|id| found[id].status != MediaStatus::Ready)
        {
            return Err(AppError::Conflict(Conflict::MediaNotReady));
        }
        let sizes = found
            .values()
            .filter_map(|media| {
                Some((
                    media.id,
                    ImageSize {
                        width: media.width?,
                        height: media.height?,
                    },
                ))
            })
            .collect();
        let rendered = self.renderer.render(&content.markdown_body, &sizes);
        Ok(NewRevision {
            id: ids::new_id(),
            content,
            rendered,
            body_media_ids: body_media,
            created_by: principal.user.id,
        })
    }
}

fn actor<'a>(principal: &Principal, meta: &'a RequestMeta) -> ContentActor<'a> {
    ContentActor {
        user_id: principal.user.id,
        meta,
    }
}

fn unknown_revision() -> AppError {
    AppError::invalid(
        "revision_id",
        FieldErrorCode::InvalidValue,
        "the revision does not belong to this post",
    )
}
