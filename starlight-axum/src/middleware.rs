use crate::meter::GLOBAL_METER;
use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName};
use axum::response::Response;
use opentelemetry::global;
use opentelemetry_http::HeaderExtractor;
use opentelemetry_instrumentation_tower::HTTPMetricsLayer;
use std::time::Duration;
use tower::ServiceBuilder;
use tower::layer::util::{Identity, Stack};
use tower_http::ServiceBuilderExt;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::request_id::{
    MakeRequestUuid, PropagateRequestId, PropagateRequestIdLayer, SetRequestId, SetRequestIdLayer,
};
use tower_http::trace::{HttpMakeClassifier, TraceLayer};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub fn generate_request_id_middleware() -> ServiceBuilder<
    Stack<PropagateRequestIdLayer, Stack<SetRequestIdLayer<MakeRequestUuid>, Identity>>,
> {
    let x_request_id = HeaderName::from_static(starlight_protocol::constants::STARLIGHT_REQUEST_ID);

    ServiceBuilder::new()
        .set_request_id(x_request_id.clone(), MakeRequestUuid)
        .propagate_request_id(x_request_id)
}

pub fn trim_slash_path() -> ServiceBuilder<Stack<NormalizePathLayer, Identity>> {
    ServiceBuilder::new().trim_trailing_slash()
}

pub fn common_middleware() -> SetRequestId<
    PropagateRequestId<ServiceBuilder<Stack<NormalizePathLayer, Identity>>>,
    MakeRequestUuid,
> {
    generate_request_id_middleware().service(trim_slash_path())
}

pub fn oltp_middleware() -> HTTPMetricsLayer {
    opentelemetry_instrumentation_tower::HTTPMetricsLayerBuilder::builder()
        .with_meter(GLOBAL_METER.clone())
        .build()
        .expect("Failed to build HTTP metrics layer")
}

pub fn trace_middleware() -> TraceLayer<
    HttpMakeClassifier,
    impl Fn(&Request<axum::body::Body>) -> Span + Clone,
    impl Fn(&Request<axum::body::Body>, &Span) + Clone,
    impl Fn(&Response<axum::body::Body>, Duration, &Span) + Clone,
    impl Fn(&Bytes, Duration, &Span) + Clone,
    impl Fn(Option<&HeaderMap>, Duration, &Span) + Clone,
    impl Fn(ServerErrorsFailureClass, Duration, &Span) + Clone,
> {
    TraceLayer::new_for_http()
        .make_span_with(|req: &Request<_>| {
            let extractor = HeaderExtractor(req.headers());
            let parent_context = global::get_text_map_propagator(|prop| prop.extract(&extractor));
            let span = tracing::info_span!("http.request", method = %req.method(), uri = %req.uri(), version = ?req.version(), headers = ?req.headers());
            span.set_parent(parent_context);
            span
        })
        .on_request(|request: &Request<_>, span: &Span| {
            let headers = format!("{:?}", request.headers());
            span.record("http.headers", &tracing::field::display(headers));
        })
        .on_response(|response: &Response<_>, latency: Duration, span: &Span| {
            span.record("http.status_code", &tracing::field::display(response.status()), );
            span.record("latency", &tracing::field::display(format!("{:?}", latency)), );
        })
        .on_body_chunk(|_chunk: &Bytes, _latency: Duration, _span: &Span| {
            // optional body logging
        })
        .on_eos(
            |_trailers: Option<&HeaderMap>, _duration: Duration, _span: &Span| {
                // end of stream
            },
        )
        .on_failure(
            |_err: ServerErrorsFailureClass, _latency: Duration, _span: &Span| {
                // error handler
            },
        )
}
