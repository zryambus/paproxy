mod app;
mod cfg;
mod router;
mod shutdown;
mod state;
mod tls;
mod tui;
mod ws;

use std::{
    net::SocketAddr, path::PathBuf, process::ExitCode, str::FromStr, sync::Arc, time::Duration,
};

use anyhow::Context;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use logroller::LogRollerBuilder;
use tokio::net::TcpListener;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    filter::{FilterExt, filter_fn},
    prelude::*,
    registry::Registry,
    util::SubscriberInitExt,
};

use cfg::{Cfg, get_config};
use router::get_router;
use shutdown::shutdown_signal;
use tui_logger::TuiTracingSubscriberLayer;

use crate::app::App;

const HEADERS_TARGET: &str = "headers";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    #[arg(long)]
    loglevel: Option<String>,
    #[arg(short, long)]
    transparent: bool,
}

async fn load_tls_config(cfg: &Cfg) -> anyhow::Result<Option<RustlsConfig>> {
    let (cert, key) = match (&cfg.certificate_path, &cfg.key_path) {
        (Some(cert), Some(key)) => (PathBuf::from(cert), PathBuf::from(key)),
        (None, None) => return Ok(None),
        _ => anyhow::bail!("Both certificate_path and key_path must be set to enable HTTPS"),
    };

    if !cert.exists() {
        anyhow::bail!("Certificate path does not exist: {}", cert.display());
    }
    if !key.exists() {
        anyhow::bail!("Key path does not exist: {}", key.display());
    }

    let config = RustlsConfig::from_pem_chain_file(cert, key)
        .await
        .context("Bad certificate/key file")?;
    Ok(Some(config))
}

async fn main_impl(cfg: Arc<Cfg>, transparent: bool) -> anyhow::Result<()> {
    tracing::info!("Logging subsystem initialized correctly");

    let mut app = App::new();
    let state = app.state();
    let router = get_router(cfg.clone(), state.clone(), transparent)?;

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind HTTP port {}", cfg.port))?;

    let https_handle = Handle::new();
    let https_server = if let Some(tls_config) = load_tls_config(&cfg).await? {
        let addr = SocketAddr::from(([0, 0, 0, 0], cfg.https_port));
        let listener = std::net::TcpListener::bind(addr)
            .with_context(|| format!("Failed to bind HTTPS port {}", cfg.https_port))?;
        listener.set_nonblocking(true)?;
        let server =
            axum_server::from_tcp_rustls(listener, tls_config)?.handle(https_handle.clone());
        let r = router.clone();

        tracing::info!(
            "Starting proxy server at https://0.0.0.0:{}",
            cfg.https_port
        );
        Some(tokio::spawn(async move {
            if let Err(e) = server.serve(r.into_make_service()).await {
                tracing::error!("HTTPS server error: {}", e);
            }
        }))
    } else {
        None
    };

    tracing::info!("Starting proxy server at http://0.0.0.0:{}", cfg.port);
    let http_server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router.into_make_service())
            .with_graceful_shutdown(shutdown_signal(state))
            .await
        {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    app.run().await?;

    // HTTP server observes the shutdown flag by itself, HTTPS server has to be told explicitly
    https_handle.graceful_shutdown(Some(SHUTDOWN_TIMEOUT));
    let servers = async {
        http_server.await.ok();
        if let Some(https_server) = https_server {
            https_server.await.ok();
        }
    };
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, servers)
        .await
        .is_err()
    {
        tracing::warn!(
            "Servers did not stop in {:?}, terminating",
            SHUTDOWN_TIMEOUT
        );
    }

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();

    let cfg = match get_config(args.config.clone()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Exiting with error: {:?}", e);
            return ExitCode::FAILURE;
        }
    };

    let filter: tui_logger::LevelFilter = args
        .loglevel
        .clone()
        .and_then(|loglevel| tui_logger::LevelFilter::from_str(&loglevel).ok())
        .unwrap_or(tui_logger::LevelFilter::Info);

    let appender = LogRollerBuilder::new("./logs", "paproxy.log")
        .rotation(logroller::Rotation::SizeBased(logroller::RotationSize::MB(
            100,
        )))
        .max_keep_files(5)
        .time_zone(logroller::TimeZone::Local)
        .graceful_shutdown(true)
        .build()
        .expect("Could not create log file");

    let (non_blocking, _guard) = tracing_appender::non_blocking(appender);

    let level_filter = LevelFilter::from_str(&args.loglevel.clone().unwrap_or("info".into()))
        .unwrap_or(LevelFilter::INFO);

    // Headers may contain cookies and tokens, so they go only to the dedicated file
    let not_headers = || filter_fn(|metadata| metadata.target() != HEADERS_TARGET);

    let fmt = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        .with_filter(level_filter.and(not_headers()));

    let mut _headers_guard = None;
    let headers_layer = cfg.log_headers.unwrap_or_default().then(|| {
        let headers_appender = LogRollerBuilder::new("./logs", "headers.json")
            .rotation(logroller::Rotation::SizeBased(logroller::RotationSize::MB(
                100,
            )))
            .max_keep_files(5)
            .time_zone(logroller::TimeZone::Local)
            .graceful_shutdown(true)
            .build()
            .expect("Could not create headers log file");

        let (headers_non_blocking, guard) = tracing_appender::non_blocking(headers_appender);
        _headers_guard = Some(guard);

        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(headers_non_blocking)
            .with_ansi(false)
            .with_filter(filter_fn(|metadata| metadata.target() == HEADERS_TARGET))
    });

    Registry::default()
        .with(TuiTracingSubscriberLayer.with_filter(not_headers()))
        .with(fmt)
        .with(headers_layer)
        .try_init()
        .expect("Could not initialize logging subsystem");

    tui_logger::init_logger(filter).expect("Failed to initialize logger");
    tui_logger::set_default_level(filter);

    let rt = tokio::runtime::Runtime::new().expect("Could not initialize Tokio runtime");
    if let Err(e) = rt.block_on(main_impl(cfg, args.transparent)) {
        tracing::error!("Exiting with error: {}", e);
        eprintln!("Exiting with error: {:?}", e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
