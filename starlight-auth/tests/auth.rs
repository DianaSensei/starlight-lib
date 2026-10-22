#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::FromRef;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use jsonwebtoken::{EncodingKey, Header, encode};
use starlight_auth::testing::{TEST_AUDIENCE, TEST_ISSUER, TestAuth};
use starlight_auth::{AuthConfig, AuthContext, AuthError, JwtVerifier};
use starlight_core::TenantId;
use starlight_web::Problem;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    verifier: Arc<JwtVerifier>,
}

impl FromRef<AppState> for Arc<JwtVerifier> {
    fn from_ref(state: &AppState) -> Self {
        Arc::clone(&state.verifier)
    }
}

async fn read_stores(auth: AuthContext) -> Result<String, Problem> {
    auth.require("merchant.store:read")?;
    Ok(auth.tenant_id().to_string())
}

fn app(auth: &TestAuth) -> Router {
    Router::new()
        .route("/stores", get(read_stores))
        .with_state(AppState {
            verifier: auth.verifier(),
        })
}

async fn call(app: Router, authorization: Option<String>) -> StatusCode {
    let mut request = Request::get("/stores");
    if let Some(value) = authorization {
        request = request.header(header::AUTHORIZATION, value);
    }
    app.oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn issued_token_verifies_with_tenant_and_permissions() {
    let auth = TestAuth::new();
    let tenant = TenantId::generate();
    let bearer = auth.bearer(tenant, &["merchant.store:read"]);

    let claims = auth
        .verifier()
        .verify(bearer.trim_start_matches("Bearer "))
        .await
        .unwrap();
    let context = AuthContext::from_claims(claims).unwrap();

    assert_eq!(context.tenant_id(), tenant);
    assert!(context.has_permission("merchant.store:read"));
    assert!(context.user_id().is_some());
}

#[tokio::test]
async fn extractor_enforces_authentication_and_permission() {
    let auth = TestAuth::new();
    let tenant = TenantId::generate();

    assert_eq!(call(app(&auth), None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        call(app(&auth), Some("Bearer not-a-jwt".into())).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(app(&auth), Some(auth.bearer(tenant, &[]))).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app(&auth),
            Some(auth.bearer(tenant, &["merchant.store:read"]))
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn token_from_another_key_is_rejected() {
    let trusted = TestAuth::new();
    let attacker = TestAuth::new();
    let forged = attacker.bearer(TenantId::generate(), &["merchant.store:read"]);

    let result = trusted
        .verifier()
        .verify(forged.trim_start_matches("Bearer "))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn wrong_audience_is_rejected() {
    let auth = TestAuth::new();
    let other_audience = AuthConfig {
        audience: "another-api".into(),
        ..TestAuth::config()
    };
    let verifier = JwtVerifier::with_jwks(other_audience, &auth.issuer().jwks()).unwrap();
    let bearer = auth.bearer(TenantId::generate(), &[]);

    let result = verifier.verify(bearer.trim_start_matches("Bearer ")).await;

    assert!(matches!(result, Err(AuthError::InvalidToken(_))));
}

#[tokio::test]
async fn symmetric_algorithms_are_refused() {
    let auth = TestAuth::new();
    let claims = serde_json::json!({
        "iss": TEST_ISSUER, "aud": TEST_AUDIENCE, "sub": "x", "exp": 4_102_444_800_u64, "iat": 0,
        "tid": TenantId::generate(),
    });
    let header = Header {
        kid: Some("test-key".into()),
        ..Header::default()
    };
    let hs256 = encode(&header, &claims, &EncodingKey::from_secret(b"guessable")).unwrap();

    let result = auth.verifier().verify(&hs256).await;

    assert!(matches!(result, Err(AuthError::UnsupportedAlgorithm(_))));
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ProviderClaims {
    iss: String,
    sub: String,
    aud: String,
    exp: u64,
    email: String,
    email_verified: bool,
}

#[tokio::test]
async fn custom_claims_of_another_issuer_are_verified() {
    let provider = TestAuth::for_issuer("https://idp.example", "starlight-identity");
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 60;
    let claims = ProviderClaims {
        iss: "https://idp.example".into(),
        sub: "idp-user-1".into(),
        aud: "starlight-identity".into(),
        exp: expires,
        email: "an@example.com".into(),
        email_verified: true,
    };
    let token = provider.issuer().sign(&claims).unwrap();

    let verified: ProviderClaims = provider.verifier().verify_as(&token).await.unwrap();
    let rejected = TestAuth::new()
        .verifier()
        .verify_as::<ProviderClaims>(&token)
        .await;

    assert_eq!(verified.email, "an@example.com");
    assert!(verified.email_verified);
    assert!(rejected.is_err());
}
