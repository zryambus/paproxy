mod cfg;
mod ws;
mod tls;
mod router;
mod shutdown;

use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing_subscriber::{prelude::*, registry::Registry, fmt};
use tracing::level_filters::LevelFilter;
use clap::Parser;

use router::get_router;
use cfg::get_config;
use shutdown::shutdown_signal;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: Option<std::path::PathBuf>,
}

async fn main_impl() -> anyhow::Result<()> {
    tracing::info!("Logging subsystem initialized correctly");

    let args = Args::parse();

    let cfg = get_config(args.config)?;
    let router = get_router(cfg.clone())?;

    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));
    let listener = TcpListener::bind(addr).await?;

    tracing::info!("Starting proxy server at http://127.0.0.1:{}", cfg.port);
    if let Err(e) = axum::serve(listener, router.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        return Err(anyhow::anyhow!(e));
    }
    Ok(())
}

fn main() {
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_filter(LevelFilter::INFO);

    Registry::default()
        .with(fmt_layer)
        .try_init()
        .expect("Could not initialize logging subsystem");

    let rt = tokio::runtime::Runtime::new().expect("Could not initialize Tokio runtime");
    if let Err(e) = rt.block_on(main_impl()) {
        tracing::error!("{}", e);
    }
}

