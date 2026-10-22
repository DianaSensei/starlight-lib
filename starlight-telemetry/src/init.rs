use std::path::PathBuf;
use std::time::Duration;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{KeyValue, global};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use opentelemetry_semantic_conventions::attribute::{DEPLOYMENT_ENVIRONMENT_NAME, SERVICE_VERSION};
use serde::Deserialize;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::ServiceInfo;
use crate::format::{JsonEventFormat, RedactingJsonFields};

const NOISY_DEPENDENCIES: &[&str] = &[
    "h2",
    "hyper",
    "hyper_util",
    "tonic",
    "tower",
    "reqwest",
    "rustls",
];
const EXPORTER_CRATES: &[&str] = &["opentelemetry", "opentelemetry_sdk", "opentelemetry_otlp"];

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    /// `tracing` filter directive, e.g. `info` or `info,starlight_merchant=debug`.
    pub log_level: String,
    pub environment: String,
    /// OTLP gRPC endpoint of the Collector; export is disabled when absent.
    pub otlp_endpoint: Option<String>,
    pub trace_sample_ratio: f64,
    pub metrics_export_interval_secs: u64,
    /// Optional rotating file output; containers should rely on stdout instead.
    pub file: Option<FileLogConfig>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            environment: "development".into(),
            otlp_endpoint: None,
            trace_sample_ratio: 1.0,
            metrics_export_interval_secs: 15,
            file: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileLogConfig {
    pub directory: PathBuf,
    #[serde(default = "default_max_files")]
    pub max_files: usize,
}

fn default_max_files() -> usize {
    7
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("invalid telemetry configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid log filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    #[error("failed to build OTLP exporter: {0}")]
    Exporter(#[from] opentelemetry_otlp::ExporterBuildError),
    #[error("failed to open log file: {0}")]
    LogFile(#[from] tracing_appender::rolling::InitError),
    #[error("failed to install subscriber: {0}")]
    Subscriber(#[from] tracing_subscriber::util::TryInitError),
}

/// Flushes and shuts down exporters when dropped; keep it alive for the whole process.
pub struct TelemetryGuard {
    tracer_provider: SdkTracerProvider,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
    _file_guard: Option<WorkerGuard>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        let _ = self.tracer_provider.shutdown();
        if let Some(provider) = &self.meter_provider {
            let _ = provider.shutdown();
        }
        if let Some(provider) = &self.logger_provider {
            let _ = provider.shutdown();
        }
    }
}

struct Exporters {
    meter_provider: SdkMeterProvider,
    logger_provider: SdkLoggerProvider,
}

/// Installs the global subscriber, tracer, meter and W3C propagator.
pub fn init(
    service: ServiceInfo,
    config: &TelemetryConfig,
) -> Result<TelemetryGuard, TelemetryError> {
    if !(0.0..=1.0).contains(&config.trace_sample_ratio) {
        return Err(TelemetryError::InvalidConfig(
            "trace_sample_ratio must be within 0..=1".into(),
        ));
    }
    global::set_text_map_propagator(TraceContextPropagator::new());

    let resource = Resource::builder()
        .with_service_name(service.name)
        .with_attributes([
            KeyValue::new(SERVICE_VERSION, service.version),
            KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, config.environment.clone()),
        ])
        .build();
    let sampler = Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
        config.trace_sample_ratio,
    )));
    let mut tracer_builder = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_sampler(sampler);

    let exporters = match &config.otlp_endpoint {
        Some(endpoint) => {
            tracer_builder = tracer_builder.with_batch_exporter(
                SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(endpoint)
                    .build()?,
            );
            Some(build_exporters(
                endpoint,
                resource,
                config.metrics_export_interval_secs,
            )?)
        }
        None => None,
    };
    // Built even without an exporter so log lines still carry trace ids.
    let tracer_provider = tracer_builder.build();
    global::set_tracer_provider(tracer_provider.clone());
    if let Some(exporters) = &exporters {
        global::set_meter_provider(exporters.meter_provider.clone());
    }

    let (file_layer, file_guard) = match &config.file {
        Some(file) => {
            let appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix(service.name)
                .filename_suffix("log")
                .max_log_files(file.max_files)
                .build(&file.directory)?;
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(json_layer(service).with_writer(writer)), Some(guard))
        }
        None => (None, None),
    };
    let otlp_log_layer = exporters.as_ref().map(|exporters| {
        OpenTelemetryTracingBridge::new(&exporters.logger_provider)
            .with_filter(exporter_feedback_filter())
    });

    tracing_subscriber::registry()
        .with(log_filter(&config.log_level)?)
        .with(tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer(service.name)))
        .with(json_layer(service).with_writer(std::io::stdout))
        .with(file_layer)
        .with(otlp_log_layer)
        .try_init()?;

    let (meter_provider, logger_provider) = match exporters {
        Some(exporters) => (
            Some(exporters.meter_provider),
            Some(exporters.logger_provider),
        ),
        None => (None, None),
    };
    Ok(TelemetryGuard {
        tracer_provider,
        meter_provider,
        logger_provider,
        _file_guard: file_guard,
    })
}

fn build_exporters(
    endpoint: &str,
    resource: Resource,
    interval_secs: u64,
) -> Result<Exporters, TelemetryError> {
    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    let reader = PeriodicReader::builder(metric_exporter)
        .with_interval(Duration::from_secs(interval_secs))
        .build();
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_reader(reader)
        .build();
    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();
    Ok(Exporters {
        meter_provider,
        logger_provider,
    })
}

fn json_layer<S>(
    service: ServiceInfo,
) -> tracing_subscriber::fmt::Layer<S, RedactingJsonFields, JsonEventFormat>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .event_format(JsonEventFormat::new(service))
        .fmt_fields(RedactingJsonFields)
}

fn log_filter(level: &str) -> Result<EnvFilter, TelemetryError> {
    let quiet_dependencies = NOISY_DEPENDENCIES
        .iter()
        .map(|target| format!("{target}=warn"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(EnvFilter::try_new(format!("{quiet_dependencies},{level}"))?)
}

/// Keeps the exporter's own logs out of the OTLP log pipeline to avoid feedback loops.
fn exporter_feedback_filter() -> Targets {
    NOISY_DEPENDENCIES.iter().chain(EXPORTER_CRATES).fold(
        Targets::new().with_default(LevelFilter::TRACE),
        |targets, target| targets.with_target(*target, LevelFilter::OFF),
    )
}
