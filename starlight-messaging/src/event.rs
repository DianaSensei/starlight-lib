use serde::{Deserialize, Serialize};
use serde_json::Value;
use starlight_core::TenantId;
use time::OffsetDateTime;
use uuid::Uuid;

const SPEC_VERSION: &str = "1.0";
const JSON_CONTENT_TYPE: &str = "application/json";
const TRACEPARENT: &str = "traceparent";

/// Domain event to publish; `event_type` doubles as the NATS subject (`starlight.<module>.<entity>.<event>.v<n>`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub event_type: &'static str,
    pub subject: String,
    pub tenant_id: TenantId,
    pub data: Value,
}

impl NewEvent {
    pub fn new(
        event_type: &'static str,
        subject: impl ToString,
        tenant_id: TenantId,
        data: impl Serialize,
    ) -> Self {
        Self {
            event_type,
            subject: subject.to_string(),
            tenant_id,
            data: serde_json::to_value(data).unwrap_or(Value::Null),
        }
    }
}

/// CloudEvents 1.0 JSON envelope with `tenantid` and distributed-tracing extensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudEvent {
    pub specversion: String,
    pub id: Uuid,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub subject: String,
    #[serde(with = "time::serde::rfc3339")]
    pub time: OffsetDateTime,
    pub datacontenttype: String,
    pub tenantid: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    pub data: Value,
}

impl CloudEvent {
    /// Envelope for `event` emitted by `source`, carrying the current trace context.
    pub fn from_new(source: &str, event: &NewEvent) -> Self {
        let trace_headers = starlight_telemetry::current_context_headers();
        Self {
            specversion: SPEC_VERSION.into(),
            id: Uuid::now_v7(),
            source: source.into(),
            event_type: event.event_type.into(),
            subject: event.subject.clone(),
            time: OffsetDateTime::now_utc(),
            datacontenttype: JSON_CONTENT_TYPE.into(),
            tenantid: event.tenant_id.as_uuid(),
            traceparent: trace_headers.get(TRACEPARENT).cloned(),
            data: event.data.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_follows_cloudevents_json_format() {
        let tenant = TenantId::generate();
        let event = NewEvent::new(
            "starlight.merchant.store.created.v1",
            "s-1",
            tenant,
            serde_json::json!({ "code": "A" }),
        );

        let json =
            serde_json::to_value(CloudEvent::from_new("/starlight/merchant", &event)).unwrap();

        assert_eq!(json["specversion"], "1.0");
        assert_eq!(json["type"], "starlight.merchant.store.created.v1");
        assert_eq!(json["source"], "/starlight/merchant");
        assert_eq!(json["subject"], "s-1");
        assert_eq!(json["tenantid"], tenant.to_string());
        assert_eq!(json["data"]["code"], "A");
        assert!(json["time"].as_str().unwrap().ends_with('Z'));
    }
}
