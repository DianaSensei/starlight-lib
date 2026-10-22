//! Axum foundation for Starlight HTTP APIs (conventions/03-rest-api.md).

pub mod context;
pub mod extract;
pub mod health;
pub mod problem;
pub mod response;
pub mod server;

mod observe;

pub use axum;
pub use extract::{IfMatch, Json, Pagination, Path, Query, ValidatedJson};
pub use health::health_router;
pub use problem::{FieldError, Problem};
pub use response::{DataResponse, PageInfo, PageResponse};
pub use server::{ServerConfig, build_app, serve, serve_listener};
