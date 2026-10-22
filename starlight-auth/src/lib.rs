//! OIDC/JWT verification via JWKS and tenant-scoped authorization (ADR-0003, ADR-0004).

mod context;
mod verifier;

#[cfg(feature = "issuer")]
pub mod issuer;
#[cfg(feature = "test-util")]
pub mod testing;

pub use context::{AuthContext, Subject};
pub use verifier::{AuthConfig, AuthError, Claims, JwtVerifier};
