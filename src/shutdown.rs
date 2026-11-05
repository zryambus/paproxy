use crate::state::State;
use std::sync::Arc;

pub async fn wait_for_shutdown(state: Arc<State>) {
    loop {
        use std::time::Duration;

        if state.is_shutdown() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(unix)]
pub async fn shutdown_signal(state: Arc<State>) {
    use std::io;
    use tokio::signal::unix::SignalKind;

    async fn terminate() -> io::Result<()> {
        tokio::signal::unix::signal(SignalKind::terminate())?
            .recv()
            .await;
        Ok(())
    }

    tokio::select! {
        _ = terminate() => {},
        _ = tokio::signal::ctrl_c() => {},
        _ = wait_for_shutdown(state.clone()) => {},
    }

    // Stop the TUI as well when the shutdown was initiated by an OS signal
    state.shutdown();
    tracing::info!("Signal received, starting graceful shutdown")
}

#[cfg(windows)]
pub async fn shutdown_signal(state: Arc<State>) {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = wait_for_shutdown(state.clone()) => {},
    }
    state.shutdown();
    tracing::info!("Signal received, starting graceful shutdown");
}
