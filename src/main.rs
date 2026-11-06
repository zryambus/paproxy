mod app;
mod cfg;
#[cfg(feature = "gui")]
mod gui;
#[cfg(feature = "gui")]
mod logbuf;
#[cfg(feature = "gui")]
mod profiles;
mod router;
mod shutdown;
mod state;
mod tls;
mod tui;
mod ws;

use std::{
    io::IsTerminal, net::SocketAddr, path::PathBuf, process::ExitCode, str::FromStr, sync::Arc,
    time::Duration,
};

use anyhow::Context;
use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use logroller::LogRollerBuilder;
use tokio::{net::TcpListener, task::JoinHandle};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    filter::{FilterExt, filter_fn},
    prelude::*,
    registry::Registry,
    util::SubscriberInitExt,
};

use cfg::{Cfg, get_config};
use router::get_router;
use shutdown::{shutdown_signal, wait_for_shutdown};
use tui_logger::TuiTracingSubscriberLayer;

use crate::{
    app::{App, dump_results},
    state::State,
};

const HEADERS_TARGET: &str = "headers";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Parser)]
struct Args {
    /// Path to a YAML configuration file [default: ./config.yml]
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    /// Log level: error, warn, info, debug or trace [default: info]
    #[arg(long)]
    loglevel: Option<String>,
    /// Proxy static routes instead of serving local static files, like `transparent: true` in the config
    #[arg(short, long)]
    transparent: bool,
    /// Run with the terminal UI (the default in builds without the graphical UI)
    #[arg(long)]
    tui: bool,
    /// Run without a UI and write logs to stdout
    #[arg(long, conflicts_with = "tui")]
    headless: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Tui,
    Headless,
    #[cfg(feature = "gui")]
    Gui,
}

impl Args {
    fn mode(&self) -> Mode {
        if self.headless {
            return Mode::Headless;
        }
        #[cfg(feature = "gui")]
        if !self.tui {
            return Mode::Gui;
        }
        Mode::Tui
    }
}

struct Servers {
    http: JoinHandle<()>,
    https: Option<JoinHandle<()>>,
    https_handle: Handle<SocketAddr>,
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

async fn start_servers(cfg: Arc<Cfg>, state: Arc<State>) -> anyhow::Result<Servers> {
    let router = get_router(cfg.clone(), state.clone(), cfg.transparent)?;

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind HTTP port {}", cfg.port))?;

    let https_handle = Handle::new();
    let https = if let Some(tls_config) = load_tls_config(&cfg).await? {
        let https_port = cfg
            .https_port
            .context("https_port must be set to enable HTTPS")?;
        let addr = SocketAddr::from(([0, 0, 0, 0], https_port));
        let listener = std::net::TcpListener::bind(addr)
            .with_context(|| format!("Failed to bind HTTPS port {}", https_port))?;
        listener.set_nonblocking(true)?;
        let server =
            axum_server::from_tcp_rustls(listener, tls_config)?.handle(https_handle.clone());
        let r = router.clone();

        tracing::info!("Starting proxy server at https://0.0.0.0:{}", https_port);
        Some(tokio::spawn(async move {
            if let Err(e) = server.serve(r.into_make_service()).await {
                tracing::error!("HTTPS server error: {}", e);
            }
        }))
    } else {
        None
    };

    tracing::info!("Starting proxy server at http://0.0.0.0:{}", cfg.port);
    let shutdown = shutdown_signal(state);
    let http = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router.into_make_service())
            .with_graceful_shutdown(shutdown)
            .await
        {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    Ok(Servers {
        http,
        https,
        https_handle,
    })
}

/// Stops the servers and writes the traffic statistics
async fn stop_servers(servers: Servers, state: &State) {
    state.shutdown();
    // HTTP server observes the shutdown flag by itself, HTTPS server has to be told explicitly
    servers
        .https_handle
        .graceful_shutdown(Some(SHUTDOWN_TIMEOUT));
    let stopped = async {
        servers.http.await.ok();
        if let Some(https) = servers.https {
            https.await.ok();
        }
    };
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, stopped)
        .await
        .is_err()
    {
        tracing::warn!(
            "Servers did not stop in {:?}, terminating",
            SHUTDOWN_TIMEOUT
        );
    }

    if let Err(e) = dump_results(state.get_info()) {
        tracing::error!("Failed to write results: {}", e);
    }
}

async fn main_impl(cfg: Arc<Cfg>, headless: bool) -> anyhow::Result<()> {
    let state = Arc::new(State::new());
    let servers = start_servers(cfg, state.clone()).await?;

    let ui_result = if headless {
        wait_for_shutdown(state.clone()).await;
        Ok(())
    } else {
        App::new(state.clone()).run().await
    };

    // Stop the servers even if the TUI failed
    stop_servers(servers, &state).await;
    ui_result
}

/// GPUI needs the main thread, so the proxy started from the GUI runs on the Tokio worker threads
#[cfg(feature = "gui")]
fn run_gui(rt: &tokio::runtime::Runtime, settings: gui::Settings, logs: logbuf::LogBuffer) {
    let quit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.spawn({
        let quit = quit.clone();
        async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                quit.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });

    let running = gui::RunningSlot::default();
    gui::run(rt.handle().clone(), settings, running.clone(), quit, logs);

    if let Some(running) = running.take() {
        rt.block_on(stop_servers(running.servers, &running.state));
    }
}

fn main() -> ExitCode {
    let args = Args::parse();

    let mode = args.mode();

    // The GUI starts with default settings, which can be fixed in the window
    let (mut cfg, cfg_error): (Cfg, Option<anyhow::Error>) = match get_config(args.config.clone()) {
        Ok(cfg) => (cfg, None),
        #[cfg(feature = "gui")]
        Err(e) if mode == Mode::Gui => (Cfg::default(), Some(e)),
        Err(e) => {
            eprintln!("Exiting with error: {:?}", e);
            return ExitCode::FAILURE;
        }
    };
    cfg.transparent |= args.transparent;

    // The TUI draws to stderr
    if mode == Mode::Tui && !std::io::stderr().is_terminal() {
        eprintln!(
            "Exiting with error: no terminal available, use --headless to run without the TUI"
        );
        return ExitCode::FAILURE;
    }

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
    // Header logging can be enabled in the GUI at any time
    #[cfg(feature = "gui")]
    let log_headers = cfg.log_headers || mode == Mode::Gui;
    #[cfg(not(feature = "gui"))]
    let log_headers = cfg.log_headers;
    let headers_layer = log_headers.then(|| {
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

    let tui_layer =
        (mode == Mode::Tui).then(|| TuiTracingSubscriberLayer.with_filter(not_headers()));

    let console_layer = (mode == Mode::Headless).then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(std::io::stdout().is_terminal())
            .with_target(false)
            .with_filter(level_filter.and(not_headers()))
    });

    #[cfg(feature = "gui")]
    let logs = logbuf::LogBuffer::default();
    #[cfg(feature = "gui")]
    let gui_layer =
        (mode == Mode::Gui).then(|| logs.layer().with_filter(level_filter.and(not_headers())));
    #[cfg(not(feature = "gui"))]
    let gui_layer: Option<tracing_subscriber::layer::Identity> = None;

    Registry::default()
        .with(tui_layer)
        .with(console_layer)
        .with(gui_layer)
        .with(fmt)
        .with(headers_layer)
        .try_init()
        .expect("Could not initialize logging subsystem");

    if mode == Mode::Tui {
        tui_logger::init_logger(filter).expect("Failed to initialize logger");
        tui_logger::set_default_level(filter);
    }

    tracing::info!("Logging subsystem initialized correctly");
    if let Some(e) = cfg_error {
        tracing::warn!("Using default settings: {:#}", e);
    }

    let rt = tokio::runtime::Runtime::new().expect("Could not initialize Tokio runtime");

    #[cfg(feature = "gui")]
    if mode == Mode::Gui {
        let settings = gui::Settings {
            cfg,
            config_path: args.config.unwrap_or_else(|| "config.yml".into()),
        };
        run_gui(&rt, settings, logs);
        return ExitCode::SUCCESS;
    }

    let result = rt.block_on(main_impl(Arc::new(cfg), mode == Mode::Headless));
    if let Err(e) = result {
        tracing::error!("Exiting with error: {}", e);
        eprintln!("Exiting with error: {:?}", e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
