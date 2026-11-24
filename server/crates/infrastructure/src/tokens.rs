//! 基于 HS256 JWT 的 access token 与 refresh token。
//!
//! 令牌只携带账户 ID 与会话 ID，不携带角色或权限；每次鉴权仍按会话读取账户当前状态，
//! 因此退出登录、停用账户或撤销会话后令牌立即失效。

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, errors::ErrorKind};
use serde::{Deserialize, Serialize};
use silva_application::ports::{
    IssuedTokens, TokenClaims, TokenKind, TokenRejection, TokenService,
};
use silva_shared::{
    config::JwtSecret,
    error::{AppError, Result},
    ids,
};
use std::time::Duration;
use uuid::Uuid;

/// 令牌签发方，对应声明 `iss`。
pub const ISSUER: &str = "silva";
/// 令牌受众，对应声明 `aud`。
pub const AUDIENCE: &str = "silva-admin";
/// 唯一接受的签名算法。
const ALGORITHM: Algorithm = Algorithm::HS256;

/// 令牌签发配置。
#[derive(Clone, Debug)]
pub struct TokenSettings {
    pub secret: JwtSecret,
    pub access_ttl: Duration,
    pub refresh_ttl: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    /// 账户 ID。
    sub: Uuid,
    /// 服务端会话 ID（`sessions.id`）。
    sid: Uuid,
    /// 令牌唯一标识，保证同一会话的每个令牌互不相同。
    jti: Uuid,
    iat: i64,
    exp: i64,
    /// 令牌种类：`access` 或 `refresh`。
    typ: String,
}

pub struct JwtTokens {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    access_ttl: Duration,
    refresh_ttl: Duration,
}

impl JwtTokens {
    pub fn new(settings: &TokenSettings) -> Self {
        let mut validation = Validation::new(ALGORITHM);
        validation.algorithms = vec![ALGORITHM];
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        // 过期在校验 `typ` 之后手动判断，使错误种类的令牌即使过期也按无效处理。
        validation.validate_exp = false;
        validation.leeway = 0;
        Self {
            encoding: EncodingKey::from_secret(settings.secret.as_bytes()),
            decoding: DecodingKey::from_secret(settings.secret.as_bytes()),
            validation,
            access_ttl: settings.access_ttl,
            refresh_ttl: settings.refresh_ttl,
        }
    }

    fn sign(
        &self,
        user_id: Uuid,
        session_id: Uuid,
        kind: TokenKind,
        issued_at: DateTime<Utc>,
        ttl: Duration,
    ) -> Result<(String, DateTime<Utc>)> {
        let lifetime = chrono::Duration::from_std(ttl)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("token lifetime too large")))?;
        // JWT 的 exp 以整秒表示；返回值截断到秒，保证响应中的过期时间与 exp 声明一致。
        let expires_at = DateTime::from_timestamp((issued_at + lifetime).timestamp(), 0)
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("token expiry out of range")))?;
        let claims = Claims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: user_id,
            sid: session_id,
            jti: ids::new_id(),
            iat: issued_at.timestamp(),
            exp: expires_at.timestamp(),
            typ: kind.as_str().into(),
        };
        let token = jsonwebtoken::encode(&Header::new(ALGORITHM), &claims, &self.encoding)
            .map_err(|error| {
                AppError::Internal(anyhow::anyhow!("token signing failed: {error}"))
            })?;
        Ok((token, expires_at))
    }
}

impl TokenService for JwtTokens {
    fn issue(&self, user_id: Uuid, session_id: Uuid) -> Result<IssuedTokens> {
        let now = Utc::now();
        let (access_token, access_expires_at) =
            self.sign(user_id, session_id, TokenKind::Access, now, self.access_ttl)?;
        let (refresh_token, refresh_expires_at) = self.sign(
            user_id,
            session_id,
            TokenKind::Refresh,
            now,
            self.refresh_ttl,
        )?;
        Ok(IssuedTokens {
            access_token,
            refresh_token,
            access_expires_at,
            refresh_expires_at,
        })
    }

    fn verify(
        &self,
        token: &str,
        kind: TokenKind,
    ) -> std::result::Result<TokenClaims, TokenRejection> {
        // 解码先校验签名与算法，再校验 iss、aud 与必填声明。
        let data = jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map_err(|error| match error.kind() {
                ErrorKind::ExpiredSignature => TokenRejection::Expired,
                _ => TokenRejection::Invalid,
            })?;
        let claims = data.claims;
        if claims.typ != kind.as_str() {
            return Err(TokenRejection::Invalid);
        }
        if claims.exp <= Utc::now().timestamp() {
            return Err(TokenRejection::Expired);
        }
        Ok(TokenClaims {
            user_id: claims.sub,
            session_id: claims.sid,
            kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    fn settings(secret: &[u8]) -> TokenSettings {
        TokenSettings {
            secret: JwtSecret::new(secret.to_vec()),
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(604_800),
        }
    }

    fn tokens() -> JwtTokens {
        JwtTokens::new(&settings(&[1u8; 32]))
    }

    fn segment(value: &serde_json::Value) -> String {
        URL_SAFE_NO_PAD.encode(value.to_string())
    }

    fn payload(token: &str) -> serde_json::Value {
        let part = token.split('.').nth(1).unwrap();
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    /// 用指定密钥签发任意声明的 HS256 令牌，模拟伪造或配置不同的签发方。
    fn forge(claims: serde_json::Value, secret: &[u8]) -> String {
        jsonwebtoken::encode(
            &Header::new(ALGORITHM),
            &claims,
            &EncodingKey::from_secret(secret),
        )
        .unwrap()
    }

    fn claims(kind: &str, exp_offset: i64) -> serde_json::Value {
        let now = Utc::now().timestamp();
        serde_json::json!({
            "iss": ISSUER, "aud": AUDIENCE,
            "sub": ids::new_id(), "sid": ids::new_id(), "jti": ids::new_id(),
            "iat": now, "exp": now + exp_offset, "typ": kind,
        })
    }

    #[test]
    fn issued_tokens_carry_required_claims_and_verify_by_kind() {
        let tokens = tokens();
        let (user, session) = (ids::new_id(), ids::new_id());
        let issued = tokens.issue(user, session).unwrap();
        assert_ne!(issued.access_token, issued.refresh_token);
        let access = payload(&issued.access_token);
        assert_eq!(access["iss"], ISSUER);
        assert_eq!(access["aud"], AUDIENCE);
        assert_eq!(access["sub"], user.to_string());
        assert_eq!(access["sid"], session.to_string());
        assert_eq!(access["typ"], "access");
        assert_eq!(
            access["exp"].as_i64().unwrap() - access["iat"].as_i64().unwrap(),
            900
        );
        let jti: Uuid = access["jti"].as_str().unwrap().parse().unwrap();
        assert_eq!(jti.get_version_num(), 7);
        assert_eq!(payload(&issued.refresh_token)["typ"], "refresh");
        assert_eq!(
            issued.access_expires_at.timestamp(),
            access["exp"].as_i64().unwrap()
        );
        assert_eq!(
            issued.refresh_expires_at.timestamp(),
            payload(&issued.refresh_token)["exp"].as_i64().unwrap()
        );
        assert_eq!(issued.access_expires_at.timestamp_subsec_nanos(), 0);
        assert_eq!(
            (issued.refresh_expires_at - issued.access_expires_at).num_seconds(),
            604_800 - 900
        );

        let verified = tokens
            .verify(&issued.access_token, TokenKind::Access)
            .unwrap();
        assert_eq!(
            verified,
            TokenClaims {
                user_id: user,
                session_id: session,
                kind: TokenKind::Access
            }
        );
        assert!(
            tokens
                .verify(&issued.refresh_token, TokenKind::Refresh)
                .is_ok()
        );
        // 两种令牌不能互换使用。
        assert_eq!(
            tokens.verify(&issued.refresh_token, TokenKind::Access),
            Err(TokenRejection::Invalid)
        );
        assert_eq!(
            tokens.verify(&issued.access_token, TokenKind::Refresh),
            Err(TokenRejection::Invalid)
        );
        // 同一会话再次签发也得到不同的令牌。
        let again = tokens.issue(user, session).unwrap();
        assert_ne!(again.refresh_token, issued.refresh_token);
    }

    #[test]
    fn expired_tokens_are_reported_separately_from_invalid_ones() {
        let tokens = tokens();
        let expired = forge(claims("access", -1), &[1u8; 32]);
        assert_eq!(
            tokens.verify(&expired, TokenKind::Access),
            Err(TokenRejection::Expired)
        );
        // 过期但种类不符或签名错误仍按无效处理。
        assert_eq!(
            tokens.verify(&expired, TokenKind::Refresh),
            Err(TokenRejection::Invalid)
        );
        let forged_expired = forge(claims("access", -1), &[2u8; 32]);
        assert_eq!(
            tokens.verify(&forged_expired, TokenKind::Access),
            Err(TokenRejection::Invalid)
        );
    }

    #[test]
    fn tampered_or_foreign_tokens_are_invalid() {
        let tokens = tokens();
        let valid = tokens.issue(ids::new_id(), ids::new_id()).unwrap();
        let invalid = |token: &str| {
            assert_eq!(
                tokens.verify(token, TokenKind::Access),
                Err(TokenRejection::Invalid),
                "{token}"
            );
        };
        // 篡改载荷后签名不再匹配。
        let mut parts: Vec<_> = valid.access_token.split('.').map(str::to_owned).collect();
        let mut body = payload(&valid.access_token);
        body["sub"] = serde_json::json!(ids::new_id());
        parts[1] = segment(&body);
        invalid(&parts.join("."));
        // 篡改签名。
        let mut signature: Vec<char> = valid.access_token.chars().collect();
        let first = valid.access_token.rfind('.').unwrap() + 1;
        signature[first] = if signature[first] == 'A' { 'B' } else { 'A' };
        invalid(&signature.into_iter().collect::<String>());
        // 其他密钥、签发方、受众或缺少声明。
        invalid(&forge(claims("access", 900), &[2u8; 32]));
        let mut wrong_issuer = claims("access", 900);
        wrong_issuer["iss"] = serde_json::json!("someone-else");
        invalid(&forge(wrong_issuer, &[1u8; 32]));
        let mut wrong_audience = claims("access", 900);
        wrong_audience["aud"] = serde_json::json!("silva-public");
        invalid(&forge(wrong_audience, &[1u8; 32]));
        let mut missing_session = claims("access", 900);
        missing_session.as_object_mut().unwrap().remove("sid");
        invalid(&forge(missing_session, &[1u8; 32]));
        for garbage in ["", "not-a-jwt", "a.b.c", "Bearer x"] {
            invalid(garbage);
        }
    }

    #[test]
    fn algorithms_other_than_hs256_are_rejected() {
        let tokens = tokens();
        let body = segment(&claims("access", 900));
        // alg=none 且无签名。
        let none = format!(
            "{}.{body}.",
            segment(&serde_json::json!({"alg":"none","typ":"JWT"}))
        );
        assert_eq!(
            tokens.verify(&none, TokenKind::Access),
            Err(TokenRejection::Invalid)
        );
        // 同一密钥的 HS512 签名也不接受。
        let hs512 = jsonwebtoken::encode(
            &Header::new(Algorithm::HS512),
            &claims("access", 900),
            &EncodingKey::from_secret(&[1u8; 32]),
        )
        .unwrap();
        assert_eq!(
            tokens.verify(&hs512, TokenKind::Access),
            Err(TokenRejection::Invalid)
        );
    }
}
