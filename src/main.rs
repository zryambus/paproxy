mod cfg;
mod ws;
mod tls;
mod router;
mod shutdown;

use std::net::SocketAddr;
use tracing_subscriber::{prelude::*, registry::Registry, fmt};
use tracing::level_filters::LevelFilter;

use router::get_router;
use cfg::get_config;
use shutdown::shutdown_signal;

async fn main_impl() -> anyhow::Result<()> {
    tracing::info!("Logging subsystem initialized correctly");

    let cfg = get_config()?;
    let router = get_router(cfg.clone())?;

    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));

    tracing::info!("Starting proxy server at http://127.0.0.1:{}", cfg.port);
    Ok(
        axum::Server::bind(&addr)
            .serve(router.into_make_service())
            .with_graceful_shutdown(shutdown_signal())
            .await?
    )
}

fn main() {
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_filter(LevelFilter::INFO);

    Registry::default()
        .with(fmt_layer)
        .try_init()
        .expect("Could not initialize logging subsystem");

    let rt = tokio::runtime::Runtime::new().unwrap();
    if let Err(e) = rt.block_on(main_impl()) {
        tracing::error!("{}", e);
    }
}

