use std::collections::HashMap;

use opentelemetry::global;
use opentelemetry::trace::TraceContextExt;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// W3C trace context of the current span, ready to put into message or HTTP headers.
pub fn current_context_headers() -> HashMap<String, String> {
    let mut carrier = HashMap::new();
    let context = tracing::Span::current().context();
    global::get_text_map_propagator(|propagator| propagator.inject_context(&context, &mut carrier));
    carrier
}

/// Remote parent context extracted from W3C trace context headers.
pub fn context_from_headers(carrier: &HashMap<String, String>) -> opentelemetry::Context {
    global::get_text_map_propagator(|propagator| propagator.extract(carrier))
}

/// Trace id of the current span, if it is being traced.
pub fn current_trace_id() -> Option<String> {
    let context = tracing::Span::current().context();
    let span = context.span();
    let span_context = span.span_context();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}
