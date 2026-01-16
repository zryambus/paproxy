mod cfg;
mod ws;
mod tls;
mod router;
mod shutdown;
mod state;
mod tui;
mod app;

use std::{net::SocketAddr, str::FromStr, sync::Arc};

use tokio::net::TcpListener;
use tracing_subscriber::{prelude::*, registry::Registry};
use clap::Parser;

use router::get_router;
use cfg::get_config;
use shutdown::shutdown_signal;
use tui_logger::TuiTracingSubscriberLayer;

use crate::app::App;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    #[arg(long)]
    loglevel: Option<String>,
    #[arg(short, long)]
    transparent: bool
}

async fn main_impl(args: Args) -> anyhow::Result<()> {
    tracing::info!("Logging subsystem initialized correctly");

    let cfg = get_config(args.config)?;
    let app = Arc::new(App::new());
    let router = get_router(cfg.clone(), app.clone(), args.transparent)?;
    let state = app.state();

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = TcpListener::bind(addr).await?;

    tokio::spawn(async move {
        tracing::info!("Starting proxy server at http://127.0.0.1:{}", cfg.port);
        axum::serve(listener, router.into_make_service())
            .with_graceful_shutdown(shutdown_signal(state))
            .await
    });

    app.run().await?;

    Ok(())
}

fn main() {
    let args = Args::parse();

    let filter: tui_logger::LevelFilter = args.loglevel
        .clone()
        .and_then(|loglevel| tui_logger::LevelFilter::from_str(&loglevel).ok())
        .unwrap_or(tui_logger::LevelFilter::Info);
    
    Registry::default()
        .with(TuiTracingSubscriberLayer)
        .try_init()
        .expect("Could not initialize logging subsystem");

    tui_logger::init_logger(filter)
        .expect("Failed to initialize logger");
    tui_logger::set_default_level(filter);

    let rt = tokio::runtime::Runtime::new().expect("Could not initialize Tokio runtime");
    if let Err(e) = rt.block_on(main_impl(args)) {
        tracing::error!("{}", e);
    }
}

