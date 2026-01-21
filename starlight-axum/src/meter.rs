use crate::get_env_or_panic;
use crate::resource::get_resource;
use opentelemetry::metrics::Meter;
use opentelemetry::{InstrumentationScope, global};
use opentelemetry_otlp::{MetricExporter, WithExportConfig};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

static SDK_METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

pub fn get_meter_provider() -> &'static SdkMeterProvider {
    SDK_METER_PROVIDER
        .get()
        .expect("failed to get meter provider")
}

pub fn get_or_init_meter_provider(oltp_grpc_url: &str) -> SdkMeterProvider {
    SDK_METER_PROVIDER
        .get_or_init(|| {
            let metric_exporter = MetricExporter::builder()
                .with_tonic()
                .with_endpoint(oltp_grpc_url)
                .with_temporality(opentelemetry_sdk::metrics::Temporality::default())
                .build()
                .expect("failed to create metric exporter");

            SdkMeterProvider::builder()
                .with_reader(
                    PeriodicReader::builder(metric_exporter)
                        .with_interval(Duration::from_secs(5))
                        .build(),
                )
                .with_resource(get_resource())
                .build()
        })
        .clone()
}

pub static GLOBAL_METER: LazyLock<Meter> = LazyLock::new(|| {
    let scope = InstrumentationScope::builder(get_env_or_panic("CARGO_PKG_NAME"))
        .with_version(get_env_or_panic("CARGO_PKG_VERSION"))
        .build();
    global::meter_with_scope(scope)
});

#[macro_export]
macro_rules! counter {
    ($metric:expr, $value:expr, $($label_key:expr => $label_value:expr),*) => {{
        use opentelemetry::metrics::Meter;
        use opentelemetry::KeyValue;

        let counter = GLOBAL_METER.f64_counter($metric.name())
            .with_description($metric.description())
            .with_unit($metric.unit())
            .build();

        let labels = vec![$(KeyValue::new($label_key, $label_value)),*];
        counter.add($value, &labels);
    }};
}

#[macro_export]
macro_rules! gauge {
    ($metric:expr, $value:expr, $($label_key:expr => $label_value:expr),*) => {{
        use opentelemetry::metrics::Meter;
        use opentelemetry::KeyValue;

        let gauge = GLOBAL_METER.f64_gauge($metric.name())
            .with_description($metric.description())
            .with_unit($metric.unit())
            .build();

        let labels = vec![$(KeyValue::new($label_key, $label_value)),*];
        gauge.record($value, &labels);
    }};
}

#[macro_export]
macro_rules! histogram {
    ($metric:expr, $value:expr, $($label_key:expr => $label_value:expr),*) => {{
        use opentelemetry::metrics::Meter;
        use opentelemetry::KeyValue;

        let histogram = GLOBAL_METER.f64_histogram($metric.name())
            .with_description($metric.description())
            .with_unit($metric.unit())
            .build();

        let labels = vec![$(KeyValue::new($label_key, $label_value)),*];
        histogram.record($value, &labels);
    }};
}

macro_rules! predefine_metrics {
    // Match multiple variants, each with name, description, and unit.
    ($($variant:ident { name: $name:expr, description: $description:expr, unit: $unit:expr }),* $(,)?) => {
        pub enum Metric {
            $($variant),*
        }

        impl Metric {
            pub fn name(&self) -> &'static str {
                match self {
                    $(Metric::$variant => $name),*
                }
            }

            pub fn description(&self) -> &'static str {
                match self {
                    $(Metric::$variant => $description),*
                }
            }

            pub fn unit(&self) -> &'static str {
                match self {
                    $(Metric::$variant => $unit),*
                }
            }
        }
    };
}

predefine_metrics! {
    HttpRequestsTotal {
        name: "http_requests_total",
        description: "Total number of HTTP requests",
        unit: "requests"
    },
    HttpRequestsDurationSeconds {
        name: "http_requests_duration_seconds",
        description: "Duration of HTTP requests in seconds",
        unit: "seconds"
    },
}
