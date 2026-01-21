use crate::{get_env_or_default, get_env_or_panic};
use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::attribute::{
    DEPLOYMENT_ENVIRONMENT_NAME, SERVICE_NAME, SERVICE_VERSION,
};
use std::sync::OnceLock;

pub fn get_resource() -> Resource {
    static RESOURCE: OnceLock<Resource> = OnceLock::new();
    RESOURCE
        .get_or_init(|| {
            Resource::builder()
                .with_service_name(get_env_or_panic("CARGO_PKG_NAME"))
                .with_attributes([
                    KeyValue::new(SERVICE_NAME, get_env_or_panic("CARGO_PKG_NAME")),
                    KeyValue::new(SERVICE_VERSION, get_env_or_panic("CARGO_PKG_VERSION")),
                    KeyValue::new(
                        DEPLOYMENT_ENVIRONMENT_NAME,
                        get_env_or_default("CARGO_ENV", "development".to_owned()),
                    ),
                ])
                .build()
        })
        .clone()
}
