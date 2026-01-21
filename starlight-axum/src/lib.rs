pub mod logger;
pub mod meter;
pub mod tracer;
pub mod resource;
pub mod oltp;
pub mod middleware;

#[macro_use]
extern crate tracing as internal_tracing;

pub use headers;
pub use axum;
pub use time;
pub use tower;
pub use tower_http;

pub(crate) fn get_env_or_panic(variable: &str) -> String {
    std::env::var(variable).expect(format!("{} is not set", variable).as_str())
}

pub(crate) fn get_env_or_default(variable: &str, default: String) -> String {
    std::env::var(variable).unwrap_or(default)
}

