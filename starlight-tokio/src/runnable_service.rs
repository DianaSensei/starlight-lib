use std::sync::Arc;
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub trait StarlightService: Send + Sync + 'static {
    fn run(&self, shutdown_tx: Arc<watch::Sender<bool>>, shutdown_rx: watch::Receiver<bool>, ) -> JoinHandle<()>;
}