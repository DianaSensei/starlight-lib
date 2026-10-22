use std::sync::LazyLock;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::KeyValue;
use opentelemetry::metrics::Histogram;
use opentelemetry_http::HeaderExtractor;
use starlight_telemetry::OpenTelemetrySpanExt;
use tracing::Instrument;
use tracing::field::Empty;

use crate::context::{self, REQUEST_ID_HEADER, RequestContext};

const UNMATCHED_ROUTE: &str = "unmatched";

static REQUEST_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    starlight_telemetry::meter("starlight-web")
        .f64_histogram("http.server.request.duration")
        .with_unit("s")
        .with_description("Duration of HTTP server requests.")
        .build()
});

/// One span, one access log line and one duration sample per request.
pub(crate) async fn observe(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(UNMATCHED_ROUTE, MatchedPath::as_str)
        .to_owned();
    let request_context = RequestContext {
        request_id: context::request_id(request.headers()),
        path: request.uri().path().to_owned(),
        locale: context::negotiate_locale(request.headers()),
    };

    let span = tracing::info_span!(
        "http.request",
        otel.name = %format!("{method} {route}"),
        otel.kind = "server",
        otel.status_code = Empty,
        http.request.method = %method,
        http.route = %route,
        url.path = %request_context.path,
        request_id = %request_context.request_id,
        http.response.status_code = Empty,
        tenant.id = Empty,
        user.id = Empty,
    );
    let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    let _ = span.set_parent(parent);

    let request_id = request_context.request_id.clone();
    let locale = request_context.locale;
    let mut response = context::scope(request_context, next.run(request))
        .instrument(span.clone())
        .await;

    let status = response.status().as_u16();
    span.record("http.response.status_code", status);
    if status >= 500 {
        span.record("otel.status_code", "ERROR");
    }
    let elapsed = started.elapsed();
    let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    tracing::info!(parent: &span, duration_ms, "request completed");
    REQUEST_DURATION.record(
        elapsed.as_secs_f64(),
        &[
            KeyValue::new("http.request.method", method),
            KeyValue::new("http.route", route),
            KeyValue::new("http.response.status_code", i64::from(status)),
        ],
    );

    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        headers.insert(REQUEST_ID_HEADER, value);
    }
    headers.insert(
        axum::http::header::CONTENT_LANGUAGE,
        HeaderValue::from_static(locale),
    );
    response
}
