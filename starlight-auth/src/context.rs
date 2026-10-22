use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header;
use axum::http::request::Parts;
use starlight_core::{TenantId, UserId};
use starlight_web::Problem;

use crate::verifier::{AuthError, Claims, JwtVerifier};

const BEARER_PREFIX: &str = "bearer ";

/// Who is calling: an end user, or a machine client acting for a consuming system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    User(UserId),
    Client(String),
}

/// Verified caller identity, tenant and permissions; extract it in every protected handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    subject: Subject,
    tenant_id: TenantId,
    permissions: BTreeSet<String>,
}

impl AuthContext {
    pub fn new(
        subject: Subject,
        tenant_id: TenantId,
        permissions: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            subject,
            tenant_id,
            permissions: permissions.into_iter().collect(),
        }
    }

    pub fn from_claims(claims: Claims) -> Result<Self, AuthError> {
        let tenant_id = claims
            .tid
            .map(TenantId::from)
            .ok_or(AuthError::MissingTenant)?;
        let subject = claims.sub.parse().map_or_else(
            |_| Subject::Client(claims.sub.clone()),
            |id| Subject::User(UserId::from_uuid(id)),
        );
        let scopes = claims
            .scope
            .iter()
            .flat_map(|scope| scope.split_whitespace())
            .map(str::to_owned);
        Ok(Self::new(
            subject,
            tenant_id,
            claims.perms.into_iter().chain(scopes),
        ))
    }

    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    pub fn subject(&self) -> &Subject {
        &self.subject
    }

    pub fn user_id(&self) -> Option<UserId> {
        match &self.subject {
            Subject::User(id) => Some(*id),
            Subject::Client(_) => None,
        }
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    /// `403 auth.forbidden` unless the caller holds `permission`.
    pub fn require(&self, permission: &str) -> Result<(), Problem> {
        if self.has_permission(permission) {
            return Ok(());
        }
        tracing::info!(permission, "permission denied");
        Err(Problem::forbidden())
    }
}

impl<S> FromRequestParts<S> for AuthContext
where
    Arc<JwtVerifier>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or_else(Problem::unauthenticated)?;
        let verifier = Arc::<JwtVerifier>::from_ref(state);
        let claims = verifier.verify(token).await.map_err(reject_token)?;
        let context = Self::from_claims(claims).map_err(reject_token)?;

        let span = tracing::Span::current();
        span.record("tenant.id", tracing::field::display(context.tenant_id));
        if let Some(user_id) = context.user_id() {
            span.record("user.id", tracing::field::display(user_id));
        }
        Ok(context)
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let has_bearer_prefix = value.len() > BEARER_PREFIX.len()
        && value[..BEARER_PREFIX.len()].eq_ignore_ascii_case(BEARER_PREFIX);
    has_bearer_prefix.then(|| value[BEARER_PREFIX.len()..].trim())
}

fn reject_token(error: AuthError) -> Problem {
    tracing::warn!(reason = %error, "token verification failed");
    Problem::new(
        axum::http::StatusCode::UNAUTHORIZED,
        "auth.invalid_token",
        "Invalid access token",
    )
}
