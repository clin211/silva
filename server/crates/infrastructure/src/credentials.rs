use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{Error as PasswordError, SaltString, rand_core::OsRng},
};
use async_trait::async_trait;
use rand::RngCore;
use sha2::{Digest, Sha256};
use silva_application::ports::Credentials;
use silva_shared::error::{AppError, Result};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct CryptoCredentials {
    dummy_hash: String,
    // 限制密码校验的并发数，控制大量登录请求同时到达时的内存消耗。
    slots: Arc<Semaphore>,
}

/// 密码实现错误保留具体类别，不把损坏的哈希或算法异常误判为密码不匹配。
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("password hashing failed: {0}")]
    Hash(PasswordError),
    #[error("invalid stored password hash: {0}")]
    InvalidHash(PasswordError),
    #[error("password verification failed: {0}")]
    Verify(PasswordError),
    #[error("password task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl CryptoCredentials {
    pub async fn new() -> std::result::Result<Self, CredentialError> {
        let dummy_hash =
            tokio::task::spawn_blocking(|| hash("unknown-account-dummy-password")).await??;
        Ok(Self {
            dummy_hash,
            slots: Arc::new(Semaphore::new(4)),
        })
    }
}

fn hash(password: &str) -> std::result::Result<String, CredentialError> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|value| value.to_string())
        .map_err(CredentialError::Hash)
}

fn verify(password: &str, encoded: &str) -> std::result::Result<bool, CredentialError> {
    let parsed = PasswordHash::new(encoded).map_err(CredentialError::InvalidHash)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(PasswordError::Password) => Ok(false),
        Err(error) => Err(CredentialError::Verify(error)),
    }
}

#[async_trait]
impl Credentials for CryptoCredentials {
    async fn hash_password(&self, password: String) -> Result<String> {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            hash(&password)
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(|error| AppError::Internal(error.into()))
    }

    async fn verify_password(
        &self,
        password: String,
        password_hash: Option<String>,
    ) -> Result<bool> {
        let known = password_hash.is_some();
        let encoded = password_hash.unwrap_or_else(|| self.dummy_hash.clone());
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            verify(&password, &encoded).map(|valid| known && valid)
        })
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(|error| AppError::Internal(error.into()))
    }

    fn new_session_token(&self) -> String {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    fn token_hash(&self, token: &str) -> String {
        hex::encode(Sha256::digest(token.as_bytes()))
    }

    fn csrf_token(&self, token: &str) -> String {
        // 使用独立的派生前缀，将 CSRF 令牌绑定到保密的 HttpOnly 会话令牌。
        let mut hasher = Sha256::new();
        hasher.update(b"silva-csrf-v1:");
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    const PASSWORD: &str = "correct-test-password";
    static PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| hash(PASSWORD).unwrap());

    #[test]
    fn verify_should_accept_matching_password() {
        assert!(verify(PASSWORD, &PASSWORD_HASH).unwrap());
    }

    #[test]
    fn verify_should_reject_wrong_password() {
        assert!(!verify("wrong-password", &PASSWORD_HASH).unwrap());
    }

    #[test]
    fn verify_should_report_malformed_hash() {
        assert!(matches!(
            verify(PASSWORD, "invalid-hash"),
            Err(CredentialError::InvalidHash(_))
        ));
    }

    #[test]
    fn verify_should_report_unsupported_algorithm() {
        let unsupported_hash = PASSWORD_HASH.replacen("$argon2id$", "$unsupported$", 1);
        assert!(matches!(
            verify(PASSWORD, &unsupported_hash),
            Err(CredentialError::Verify(PasswordError::Algorithm))
        ));
    }

    #[tokio::test]
    async fn verify_password_should_preserve_internal_failure() {
        let credentials = CryptoCredentials::new().await.unwrap();
        let result = credentials
            .verify_password(PASSWORD.into(), Some("invalid-hash".into()))
            .await;
        assert!(matches!(result, Err(AppError::Internal(_))));
    }

    #[tokio::test]
    async fn verify_password_should_reject_unknown_account_even_when_dummy_password_matches() {
        let credentials = CryptoCredentials::new().await.unwrap();
        assert!(
            !credentials
                .verify_password("unknown-account-dummy-password".into(), None)
                .await
                .unwrap()
        );
    }
}
