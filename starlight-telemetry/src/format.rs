use std::fmt;

use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::field::{Field, Visit};
use tracing::span::Record;
use tracing::{Event, Subscriber};
use tracing_opentelemetry::OtelData;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

use crate::ServiceInfo;

const REDACTED: &str = "[REDACTED]";
const MESSAGE_FIELD: &str = "message";
const OTEL_FIELD_PREFIX: &str = "otel.";
const SENSITIVE_MARKERS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "authorization",
    "cookie",
    "otp",
    "api_key",
    "apikey",
    "private_key",
    "credential",
];

pub(crate) fn is_sensitive(field_name: &str) -> bool {
    let lowered = field_name.to_ascii_lowercase();
    SENSITIVE_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

#[derive(Default)]
struct JsonVisitor {
    fields: Map<String, Value>,
    message: Option<String>,
}

impl JsonVisitor {
    fn insert(&mut self, field: &Field, value: Value) {
        let name = field.name();
        if name == MESSAGE_FIELD {
            self.message = Some(
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned),
            );
            return;
        }
        if name.starts_with(OTEL_FIELD_PREFIX) {
            return;
        }
        let value = if is_sensitive(name) {
            Value::from(REDACTED)
        } else {
            value
        };
        self.fields.insert(name.to_owned(), value);
    }
}

impl Visit for JsonVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let number = serde_json::Number::from_f64(value)
            .map_or_else(|| Value::from(value.to_string()), Value::Number);
        self.insert(field, number);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, Value::from(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.insert(field, Value::from(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field, Value::from(format!("{value:?}")));
    }
}

/// Stores span fields as a redacted JSON object so events can merge them.
pub(crate) struct RedactingJsonFields;

impl<'writer> FormatFields<'writer> for RedactingJsonFields {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut visitor = JsonVisitor::default();
        fields.record(&mut visitor);
        write!(writer, "{}", Value::Object(visitor.fields))
    }

    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &Record<'_>,
    ) -> fmt::Result {
        let mut merged: Map<String, Value> =
            serde_json::from_str(&current.fields).unwrap_or_default();
        let mut visitor = JsonVisitor::default();
        fields.record(&mut visitor);
        merged.extend(visitor.fields);
        current.fields = Value::Object(merged).to_string();
        Ok(())
    }
}

/// One JSON object per line: timestamp, level, message, trace ids, span fields, event fields.
#[derive(Clone, Copy)]
pub(crate) struct JsonEventFormat {
    service: ServiceInfo,
}

impl JsonEventFormat {
    pub(crate) fn new(service: ServiceInfo) -> Self {
        Self { service }
    }
}

impl<S, N> FormatEvent<S, N> for JsonEventFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let mut record = Map::new();
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        record.insert("timestamp".into(), Value::from(timestamp));
        record.insert("level".into(), Value::from(metadata.level().as_str()));
        record.insert("target".into(), Value::from(metadata.target()));
        record.insert("service.name".into(), Value::from(self.service.name));
        record.insert("service.version".into(), Value::from(self.service.version));

        if let Some(scope) = ctx.event_scope() {
            let mut leaf_name = None;
            for span in scope.from_root() {
                leaf_name = Some(span.name());
                let extensions = span.extensions();
                if let Some(otel) = extensions.get::<OtelData>() {
                    insert_trace_ids(&mut record, otel);
                }
                if let Some(stored) = extensions.get::<FormattedFields<N>>() {
                    let span_fields: Map<String, Value> =
                        serde_json::from_str(&stored.fields).unwrap_or_default();
                    record.extend(span_fields);
                }
            }
            if let Some(name) = leaf_name {
                record.insert("span".into(), Value::from(name));
            }
        }

        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        record.extend(visitor.fields);
        record.insert(
            MESSAGE_FIELD.into(),
            Value::from(visitor.message.unwrap_or_default()),
        );

        writeln!(writer, "{}", Value::Object(record))
    }
}

fn insert_trace_ids(record: &mut Map<String, Value>, otel: &OtelData) {
    let trace_id = otel
        .trace_id()
        .filter(|id| *id != opentelemetry::TraceId::INVALID);
    let span_id = otel
        .span_id()
        .filter(|id| *id != opentelemetry::SpanId::INVALID);
    if let Some(trace_id) = trace_id {
        record.insert("trace_id".into(), Value::from(trace_id.to_string()));
    }
    if let Some(span_id) = span_id {
        record.insert("span_id".into(), Value::from(span_id.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture(emit: impl FnOnce()) -> Value {
        let buffer = Buffer::default();
        let service = ServiceInfo {
            name: "test-service",
            version: "1.0.0",
        };
        let layer = tracing_subscriber::fmt::layer()
            .event_format(JsonEventFormat::new(service))
            .fmt_fields(RedactingJsonFields)
            .with_writer(buffer.clone());
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, emit);
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        serde_json::from_str(output.trim()).unwrap()
    }

    #[test]
    fn event_is_json_with_message_and_fields() {
        let line = capture(|| tracing::info!(store.id = "s-1", attempts = 3, "store created"));

        assert_eq!(line["message"], "store created");
        assert_eq!(line["level"], "INFO");
        assert_eq!(line["store.id"], "s-1");
        assert_eq!(line["attempts"], 3);
        assert_eq!(line["service.name"], "test-service");
    }

    #[test]
    fn sensitive_fields_are_redacted() {
        let line = capture(|| {
            tracing::warn!(
                password = "hunter2",
                refresh_token = "abc",
                user.id = "u-1",
                "login failed"
            )
        });

        assert_eq!(line["password"], REDACTED);
        assert_eq!(line["refresh_token"], REDACTED);
        assert_eq!(line["user.id"], "u-1");
    }

    #[test]
    fn span_fields_are_attached_and_redacted() {
        let line = capture(|| {
            let span = tracing::info_span!(
                "http.request",
                request_id = "r-1",
                api_key = "k",
                status = tracing::field::Empty
            );
            let _entered = span.enter();
            span.record("status", 200);
            tracing::info!("request completed");
        });

        assert_eq!(line["request_id"], "r-1");
        assert_eq!(line["api_key"], REDACTED);
        assert_eq!(line["status"], 200);
        assert_eq!(line["span"], "http.request");
    }
}
