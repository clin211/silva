//! 整体保存个人资料。

use crate::ports::{ContentActor, RequestMeta};
use async_trait::async_trait;
use silva_domain::{identity::Principal, profile::Profile, site::SiteVersion};
use silva_shared::error::Result;
use std::sync::Arc;

#[async_trait]
pub trait ProfileRepository: Send + Sync {
    async fn save(&self, profile: &Profile, actor: &ContentActor<'_>) -> Result<SiteVersion>;
}

pub struct ProfileService {
    repository: Arc<dyn ProfileRepository>,
}

impl ProfileService {
    pub fn new(repository: Arc<dyn ProfileRepository>) -> Self {
        Self { repository }
    }

    pub async fn save(
        &self,
        principal: &Principal,
        profile: Profile,
        meta: &RequestMeta,
    ) -> Result<SiteVersion> {
        principal.require("profile.manage")?;
        profile.validate()?;
        self.repository
            .save(
                &profile,
                &ContentActor {
                    user_id: principal.user.id,
                    meta,
                },
            )
            .await
    }
}
