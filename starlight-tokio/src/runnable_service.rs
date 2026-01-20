use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait StarlightService: Send + Sync + 'static {
    async fn run(&self, shutdown: CancellationToken ) -> anyhow::Result<()>;
}