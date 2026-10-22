use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use starlight_core::{TenantId, UserId};

use crate::issuer::{IssuerSettings, TokenIssuer};
use crate::verifier::{AuthConfig, JwtVerifier};

pub const TEST_ISSUER: &str = "https://identity.starlight.test";
pub const TEST_AUDIENCE: &str = "starlight";
const TEST_KEY_ID: &str = "test-key";

/// Freshly generated key pair: mint tokens and verify them without a running identity service.
pub struct TestAuth {
    issuer: TokenIssuer,
    verifier: Arc<JwtVerifier>,
    config: AuthConfig,
}

impl TestAuth {
    pub fn new() -> Self {
        Self::for_issuer(TEST_ISSUER, TEST_AUDIENCE)
    }

    /// Key pair for another issuer, e.g. a stand-in for an external identity provider.
    #[expect(clippy::expect_used, reason = "test fixture setup")]
    pub fn for_issuer(issuer: &str, audience: &str) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let settings = IssuerSettings {
            issuer: issuer.into(),
            audience: audience.into(),
            key_id: TEST_KEY_ID.into(),
            access_token_ttl: Duration::from_secs(300),
        };
        let config = AuthConfig {
            issuer: issuer.into(),
            audience: audience.into(),
            jwks_url: String::new(),
            jwks_cache_secs: 300,
        };
        let issuer =
            TokenIssuer::from_signing_key(&signing_key, settings).expect("valid generated key");
        let verifier =
            JwtVerifier::with_jwks(config.clone(), &issuer.jwks()).expect("valid verifier");
        Self {
            issuer,
            verifier: Arc::new(verifier),
            config,
        }
    }

    pub fn config() -> AuthConfig {
        AuthConfig {
            issuer: TEST_ISSUER.into(),
            audience: TEST_AUDIENCE.into(),
            jwks_url: String::new(),
            jwks_cache_secs: 300,
        }
    }

    pub fn auth_config(&self) -> AuthConfig {
        self.config.clone()
    }

    pub fn issuer(&self) -> &TokenIssuer {
        &self.issuer
    }

    pub fn verifier(&self) -> Arc<JwtVerifier> {
        Arc::clone(&self.verifier)
    }

    /// `Authorization` header value for a new user in `tenant_id` holding `permissions`.
    #[expect(clippy::expect_used, reason = "test fixture setup")]
    pub fn bearer(&self, tenant_id: TenantId, permissions: &[&str]) -> String {
        let permissions: Vec<String> = permissions
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect();
        let token = self
            .issuer
            .issue(&UserId::generate().to_string(), tenant_id, &permissions)
            .expect("token signs");
        format!("Bearer {}", token.access_token)
    }
}

impl Default for TestAuth {
    fn default() -> Self {
        Self::new()
    }
}
