pub mod config;
pub mod error;
pub mod ids;
pub mod metrics;
pub mod pagination;
pub mod telemetry;
pub mod time;

pub async fn shutdown_signal() {
    match wait_for_shutdown().await {
        Ok(()) => tracing::info!("shutdown requested"),
        Err(error) => tracing::error!(%error, "shutdown signal handler failed; shutting down"),
    }
}

async fn wait_for_shutdown() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            result = terminate.recv() => result.ok_or_else(|| {
                std::io::Error::other("SIGTERM signal stream closed unexpectedly")
            }),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}
