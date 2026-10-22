use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Map, Value, json};
use starlight_core::health::HealthCheck;

const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

type Checks = Arc<Vec<Arc<dyn HealthCheck>>>;

/// `/health/live` (process is up) and `/health/ready` (dependencies reachable).
pub fn health_router(checks: Vec<Arc<dyn HealthCheck>>) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({ "status": "ok" })) }),
        )
        .route("/health/ready", get(ready))
        .with_state(Arc::new(checks))
}

async fn ready(State(checks): State<Checks>) -> Response {
    let mut results = Map::new();
    let mut is_ready = true;
    for check in checks.iter() {
        let outcome = tokio::time::timeout(CHECK_TIMEOUT, check.check()).await;
        let is_healthy = match outcome {
            Ok(Ok(())) => true,
            Ok(Err(reason)) => {
                tracing::warn!(health.check = check.name(), reason = %reason, "health check failed");
                false
            }
            Err(_) => {
                tracing::warn!(health.check = check.name(), "health check timed out");
                false
            }
        };
        is_ready &= is_healthy;
        results.insert(
            check.name().to_owned(),
            Value::from(if is_healthy { "ok" } else { "failed" }),
        );
    }
    let status = if is_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = json!({ "status": if is_ready { "ok" } else { "unavailable" }, "checks": results });
    (status, Json(body)).into_response()
}
