use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const ALLOWED_ALGORITHMS: &[Algorithm] = &[Algorithm::EdDSA, Algorithm::ES256, Algorithm::RS256];
const CLOCK_SKEW_LEEWAY_SECS: u64 = 30;
// Unknown `kid`s trigger a refresh at most this often, so random kids cannot flood the IdP.
const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const JWKS_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub issuer: String,
    pub audience: String,
    /// JWKS endpoint of the identity provider; empty when keys are provided statically.
    #[serde(default)]
    pub jwks_url: String,
    #[serde(default = "default_jwks_cache_secs")]
    pub jwks_cache_secs: u64,
}

fn default_jwks_cache_secs() -> u64 {
    300
}

/// Claims Starlight relies on; `tid` and `perms` are issued by the identity service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tid: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("token header has no key id")]
    MissingKeyId,
    #[error("token algorithm {0:?} is not allowed")]
    UnsupportedAlgorithm(Algorithm),
    #[error("no signing key with id {0}")]
    UnknownKey(String),
    #[error("signing key algorithm does not match token algorithm")]
    AlgorithmMismatch,
    #[error("token rejected: {0}")]
    InvalidToken(#[from] jsonwebtoken::errors::Error),
    #[error("failed to fetch signing keys: {0}")]
    KeyFetch(String),
    #[error("token has no tenant")]
    MissingTenant,
}

#[derive(Clone)]
struct SigningKey {
    key: DecodingKey,
    algorithm: Algorithm,
}

#[derive(Default)]
struct KeyCache {
    keys: HashMap<String, SigningKey>,
    fetched_at: Option<Instant>,
}

/// Verifies access tokens against the identity provider's JWKS, caching keys between refreshes.
pub struct JwtVerifier {
    config: AuthConfig,
    http: reqwest::Client,
    cache: RwLock<KeyCache>,
}

impl JwtVerifier {
    pub fn new(config: AuthConfig) -> Result<Self, AuthError> {
        let http = reqwest::Client::builder()
            .timeout(JWKS_FETCH_TIMEOUT)
            .build()
            .map_err(|error| AuthError::KeyFetch(error.to_string()))?;
        Ok(Self {
            config,
            http,
            cache: RwLock::new(KeyCache::default()),
        })
    }

    /// Verifier with fixed keys and no remote refresh.
    pub fn with_jwks(config: AuthConfig, jwks: &JwkSet) -> Result<Self, AuthError> {
        let verifier = Self::new(AuthConfig {
            jwks_url: String::new(),
            ..config
        })?;
        verifier.store_keys(jwks);
        Ok(verifier)
    }

    pub async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        self.verify_as(token).await
    }

    /// Verifies like [`Self::verify`] but decodes the payload into caller-defined claims, e.g. an IdP's `email`.
    pub async fn verify_as<T: DeserializeOwned>(&self, token: &str) -> Result<T, AuthError> {
        let header = decode_header(token)?;
        if !ALLOWED_ALGORITHMS.contains(&header.alg) {
            return Err(AuthError::UnsupportedAlgorithm(header.alg));
        }
        let key_id = header.kid.ok_or(AuthError::MissingKeyId)?;
        let signing_key = self.signing_key(&key_id).await?;
        if signing_key.algorithm != header.alg {
            return Err(AuthError::AlgorithmMismatch);
        }

        let mut validation = Validation::new(signing_key.algorithm);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = CLOCK_SKEW_LEEWAY_SECS;
        Ok(decode::<T>(token, &signing_key.key, &validation)?.claims)
    }

    async fn signing_key(&self, key_id: &str) -> Result<SigningKey, AuthError> {
        if let Some(key) = self.cached_key(key_id, self.cache_ttl()) {
            return Ok(key);
        }
        if self.can_refresh() {
            let jwks = self.fetch_jwks().await?;
            self.store_keys(&jwks);
        }
        self.cached_key(key_id, None)
            .ok_or_else(|| AuthError::UnknownKey(key_id.to_owned()))
    }

    fn cache_ttl(&self) -> Option<Duration> {
        let is_remote = !self.config.jwks_url.is_empty();
        is_remote.then(|| Duration::from_secs(self.config.jwks_cache_secs))
    }

    fn cached_key(&self, key_id: &str, max_age: Option<Duration>) -> Option<SigningKey> {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let is_fresh = match (max_age, cache.fetched_at) {
            (None, _) => true,
            (Some(max_age), Some(fetched_at)) => fetched_at.elapsed() < max_age,
            (Some(_), None) => false,
        };
        if is_fresh {
            cache.keys.get(key_id).cloned()
        } else {
            None
        }
    }

    fn can_refresh(&self) -> bool {
        if self.config.jwks_url.is_empty() {
            return false;
        }
        let cache = self
            .cache
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .fetched_at
            .is_none_or(|fetched_at| fetched_at.elapsed() >= MIN_REFRESH_INTERVAL)
    }

    async fn fetch_jwks(&self) -> Result<JwkSet, AuthError> {
        let response = self
            .http
            .get(&self.config.jwks_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| AuthError::KeyFetch(error.to_string()))?;
        response
            .json::<JwkSet>()
            .await
            .map_err(|error| AuthError::KeyFetch(error.to_string()))
    }

    fn store_keys(&self, jwks: &JwkSet) {
        let keys: HashMap<String, SigningKey> =
            jwks.keys.iter().filter_map(signing_key_from_jwk).collect();
        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.keys = keys;
        cache.fetched_at = Some(Instant::now());
    }
}

fn signing_key_from_jwk(jwk: &Jwk) -> Option<(String, SigningKey)> {
    let key_id = jwk.common.key_id.clone()?;
    let algorithm = match jwk.common.key_algorithm {
        Some(declared) => declared.to_string().parse().ok()?,
        None => inferred_algorithm(&jwk.algorithm)?,
    };
    if !ALLOWED_ALGORITHMS.contains(&algorithm) {
        return None;
    }
    let key = DecodingKey::from_jwk(jwk).ok()?;
    Some((key_id, SigningKey { key, algorithm }))
}

fn inferred_algorithm(parameters: &AlgorithmParameters) -> Option<Algorithm> {
    match parameters {
        AlgorithmParameters::OctetKeyPair(_) => Some(Algorithm::EdDSA),
        AlgorithmParameters::EllipticCurve(ec) if ec.curve == EllipticCurve::P256 => {
            Some(Algorithm::ES256)
        }
        AlgorithmParameters::RSA(_) => Some(Algorithm::RS256),
        AlgorithmParameters::EllipticCurve(_) | AlgorithmParameters::OctetKey(_) => None,
    }
}
