use crate::logger::{CustomLogFormatter, get_logger_provider, get_or_init_logger_provider};
use crate::meter::{get_meter_provider, get_or_init_meter_provider};
use crate::tracer::{get_or_init_tracer_provider, get_tracer_provider};
use crate::{get_env_or_default, get_env_or_panic};
use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::error::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn config_oltp(
    oltp_grpc_url: &str,
) -> Result<WorkerGuard, Box<dyn Error + Send + Sync + 'static>> {
    let tracer_provider = get_or_init_tracer_provider(oltp_grpc_url);
    let logger_provider = get_or_init_logger_provider(oltp_grpc_url);
    let meter_provider = get_or_init_meter_provider(oltp_grpc_url);
    global::set_tracer_provider(tracer_provider.clone());
    global::set_meter_provider(meter_provider.clone());

    let tracer = tracer_provider.tracer(get_env_or_panic("CARGO_PKG_NAME"));
    // Create a new OpenTelemetryTracingBridge using the above LoggerProvider.
    let layer = OpenTelemetryTracingBridge::new(&logger_provider);

    let file_appender =
        tracing_appender::rolling::minutely(".logs", get_env_or_panic("CARGO_PKG_NAME"));
    let (nonblocking_file, _guard_file) = tracing_appender::non_blocking(file_appender);

    let file_logger = tracing_subscriber::fmt::layer()
        .event_format(CustomLogFormatter)
        .with_writer(nonblocking_file);

    let console_logger = tracing_subscriber::fmt::layer()
        .event_format(CustomLogFormatter)
        .with_writer(std::io::stdout);

    let log_level_filter = EnvFilter::new(get_env_or_default(
        "RUST_LOG",
        "debug,axum_web_server=debug,tower_http=trace".to_owned(),
    ));

    global::set_text_map_propagator(TraceContextPropagator::new());
    tracing_subscriber::registry()
        .with(log_level_filter)
        .with(file_logger)
        .with(console_logger)
        .with(layer)
        .with(MetricsLayer::new(meter_provider))
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    Ok(_guard_file)
}

pub fn shutdown_oltp() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    get_tracer_provider().shutdown()?;
    get_meter_provider().shutdown()?;
    get_logger_provider().shutdown()?;
    Ok(())
}
