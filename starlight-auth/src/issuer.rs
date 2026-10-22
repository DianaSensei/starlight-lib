use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, Jwk, JwkSet, KeyAlgorithm,
    OctetKeyPairParameters, OctetKeyPairType, PublicKeyUse,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use starlight_core::TenantId;

/// RFC 9068 media type for JWT access tokens.
const ACCESS_TOKEN_TYPE: &str = "at+jwt";

#[derive(Debug, Clone)]
pub struct IssuerSettings {
    pub issuer: String,
    pub audience: String,
    pub key_id: String,
    pub access_token_ttl: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum IssuerError {
    #[error("invalid signing key: {0}")]
    InvalidKey(String),
    #[error("failed to sign token: {0}")]
    Signing(#[from] jsonwebtoken::errors::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedToken {
    pub access_token: String,
    pub token_id: String,
    pub expires_in: u64,
}

#[derive(Serialize)]
struct AccessTokenClaims<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    exp: u64,
    iat: u64,
    nbf: u64,
    jti: &'a str,
    tid: uuid::Uuid,
    perms: &'a [String],
}

/// Signs EdDSA access tokens and publishes the matching public JWK.
pub struct TokenIssuer {
    settings: IssuerSettings,
    encoding_key: EncodingKey,
    jwk: Jwk,
}

impl TokenIssuer {
    pub fn from_pkcs8_pem(pem: &str, settings: IssuerSettings) -> Result<Self, IssuerError> {
        let signing_key = SigningKey::from_pkcs8_pem(pem)
            .map_err(|error| IssuerError::InvalidKey(error.to_string()))?;
        Self::from_signing_key(&signing_key, settings)
    }

    pub(crate) fn from_signing_key(
        signing_key: &SigningKey,
        settings: IssuerSettings,
    ) -> Result<Self, IssuerError> {
        let der = signing_key
            .to_pkcs8_der()
            .map_err(|error| IssuerError::InvalidKey(error.to_string()))?;
        let encoding_key = EncodingKey::from_ed_der(der.as_bytes());
        let jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::EdDSA),
                key_id: Some(settings.key_id.clone()),
                ..CommonParameters::default()
            },
            algorithm: AlgorithmParameters::OctetKeyPair(OctetKeyPairParameters {
                key_type: OctetKeyPairType::OctetKeyPair,
                curve: EllipticCurve::Ed25519,
                x: URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes()),
            }),
        };
        Ok(Self {
            settings,
            encoding_key,
            jwk,
        })
    }

    pub fn issue(
        &self,
        subject: &str,
        tenant_id: TenantId,
        permissions: &[String],
    ) -> Result<IssuedToken, IssuerError> {
        let issued_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_in = self.settings.access_token_ttl.as_secs();
        let token_id = uuid::Uuid::now_v7().to_string();
        let claims = AccessTokenClaims {
            iss: &self.settings.issuer,
            sub: subject,
            aud: &self.settings.audience,
            exp: issued_at + expires_in,
            iat: issued_at,
            nbf: issued_at,
            jti: &token_id,
            tid: tenant_id.as_uuid(),
            perms: permissions,
        };
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.settings.key_id.clone());
        header.typ = Some(ACCESS_TOKEN_TYPE.to_owned());
        let access_token = encode(&header, &claims, &self.encoding_key)?;
        Ok(IssuedToken {
            access_token,
            token_id,
            expires_in,
        })
    }

    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: vec![self.jwk.clone()],
        }
    }
}
