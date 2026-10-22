use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower::{BoxError, ServiceBuilder};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::context::REQUEST_ID_HEADER;
use crate::observe::observe;
use crate::problem::Problem;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
    pub request_timeout_secs: u64,
    pub body_limit_bytes: usize,
    pub max_concurrent_requests: usize,
    /// Exact origins allowed for browsers; empty disables CORS.
    pub cors_allowed_origins: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 8080,
            request_timeout_secs: 30,
            body_limit_bytes: 1024 * 1024,
            max_concurrent_requests: 1024,
            cors_allowed_origins: Vec::new(),
        }
    }
}

/// Wraps API routes with the shared middleware stack; `health` stays outside it (no access logs).
pub fn build_app(api: Router, health: Router, config: &ServerConfig) -> Router {
    let api = api
        .fallback(|| async { Problem::not_found("route.not_found") })
        .method_not_allowed_fallback(|| async {
            Problem::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "route.method_not_allowed",
                "Method not allowed",
            )
        })
        .layer(
            ServiceBuilder::new()
                .layer(axum::middleware::from_fn(observe))
                .layer(CatchPanicLayer::custom(|_| panic_response()))
                .layer(HandleErrorLayer::new(overload_response))
                .load_shed()
                .concurrency_limit(config.max_concurrent_requests)
                .timeout(Duration::from_secs(config.request_timeout_secs))
                .layer(DefaultBodyLimit::max(config.body_limit_bytes))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-store"),
                )),
        );
    let api = match cors_layer(&config.cors_allowed_origins) {
        Some(cors) => api.layer(cors),
        None => api,
    };
    api.merge(health)
}

pub async fn serve(
    app: Router,
    config: &ServerConfig,
    shutdown: CancellationToken,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(SocketAddr::new(config.host, config.port)).await?;
    serve_listener(listener, app, shutdown).await
}

pub async fn serve_listener(
    listener: TcpListener,
    app: Router,
    shutdown: CancellationToken,
) -> std::io::Result<()> {
    tracing::info!(server.address = %listener.local_addr()?, "http server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { shutdown.cancelled().await })
    .await?;
    tracing::info!("http server stopped");
    Ok(())
}

fn panic_response() -> Response {
    tracing::error!("handler panicked");
    Problem::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal.error",
        "Internal server error",
    )
    .into_response()
}

async fn overload_response(error: BoxError) -> Response {
    let is_timeout = error.is::<tower::timeout::error::Elapsed>();
    let code = if is_timeout {
        "service.timeout"
    } else {
        "service.overloaded"
    };
    tracing::warn!(error.message = %error, "request rejected");
    Problem::service_unavailable(code).into_response()
}

fn cors_layer(origins: &[String]) -> Option<CorsLayer> {
    if origins.is_empty() {
        return None;
    }
    let allowed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();
    let custom_headers = [REQUEST_ID_HEADER, "idempotency-key"].map(HeaderName::from_static);
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(allowed))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers(
                [
                    header::AUTHORIZATION,
                    header::CONTENT_TYPE,
                    header::ACCEPT_LANGUAGE,
                    header::IF_MATCH,
                ]
                .into_iter()
                .chain(custom_headers)
                .collect::<Vec<_>>(),
            )
            .expose_headers([
                header::ETAG,
                header::LOCATION,
                HeaderName::from_static(REQUEST_ID_HEADER),
            ])
            .max_age(Duration::from_secs(600)),
    )
}
