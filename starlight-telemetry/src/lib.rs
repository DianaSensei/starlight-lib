//! Logs, traces and metrics for Starlight services (ADR-0011).

mod format;
mod init;
mod propagation;

pub use init::{FileLogConfig, TelemetryConfig, TelemetryError, TelemetryGuard, init};
pub use opentelemetry;
pub use propagation::{context_from_headers, current_context_headers, current_trace_id};
pub use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Identity reported in every log line and in the OTel resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceInfo {
    pub name: &'static str,
    pub version: &'static str,
}

/// [`ServiceInfo`] of the calling crate, resolved at compile time.
#[macro_export]
macro_rules! service_info {
    () => {
        $crate::ServiceInfo {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
        }
    };
}

/// Meter from the global provider; create instruments once and reuse them.
pub fn meter(scope: &'static str) -> opentelemetry::metrics::Meter {
    opentelemetry::global::meter(scope)
}
