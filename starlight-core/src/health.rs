use std::future::Future;
use std::pin::Pin;

pub type HealthFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// Dependency probe used by the readiness endpoint.
pub trait HealthCheck: Send + Sync {
    fn name(&self) -> &'static str;

    fn check(&self) -> HealthFuture<'_>;
}
