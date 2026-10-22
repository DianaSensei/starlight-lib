use tokio_util::sync::CancellationToken;

/// Token cancelled on SIGINT or SIGTERM; pass clones to every long-running task.
pub fn signal_token() -> CancellationToken {
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        tracing::info!("shutdown signal received");
        trigger.cancel();
    });
    token
}

async fn wait_for_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %error, "failed to listen for ctrl-c");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(error = %error, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
