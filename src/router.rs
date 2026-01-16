use std::{sync::Arc, path::PathBuf};

use anyhow::Context;
use axum::{
    Router, extract::{Extension, Request, WebSocketUpgrade, ws::WebSocket}, response::{IntoResponse, Response}, routing::get
};
use futures_util::{SinkExt, StreamExt};
use http::header;
use http_body_util::BodyExt;
use hyper::{StatusCode, Uri};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::handshake::client::generate_key};
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::{
    app::App, cfg::Cfg, state::State, tls::{HTTPSClient, build_client_config, build_https_client}, ws::{axum_to_tungstein, tungstein_to_axum}
};

async fn handler(
    Extension(client): Extension<HTTPSClient>,
    Extension(cfg): Extension<Arc<Cfg>>,
    Extension(app): Extension<Arc<App>>,
    req: Request
) -> std::result::Result<axum::response::Response, StatusCode> {
    async fn handler_impl(
        client: HTTPSClient,
        cfg: Arc<Cfg>,
        state: Arc<State>,
        mut req: Request,
    ) -> anyhow::Result<axum::response::Response> {
        let path = req.uri().path().to_owned();
        let path_query = req
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or(&path);
    
        let uri = format!("https://{}{}", cfg.host, path_query);
        tracing::info!("{} {}", req.method(), uri);
        
        *req.uri_mut() = Uri::try_from(uri)?.into();

        let headers = req.headers_mut();
        if headers.contains_key(http::header::HOST) {
            headers.insert(http::header::HOST, cfg.host.parse()?);
        }

        let content_length = headers.get(header::CONTENT_LENGTH)
            .and_then(|hv| hv.to_str().ok())
            .and_then(|str| u64::from_str_radix(str, 10).ok());

        if let Some(content_length) = content_length {
            state.update_sent(&path, content_length)
        };
        
        let response = client.request(req).await?;

        let (parts, incoming) = response.into_parts();
       
        let inspected_incoming = incoming.map_frame(move |frame| {
            if let Some(chunk) = frame.data_ref() {
                let len = chunk.len();
                state.update_received(&path, len as u64);
            }
            frame
        });

        let mut r = Response::new(inspected_incoming).into_response();
        *r.headers_mut() = parts.headers;
        *r.status_mut() = parts.status;
        *r.version_mut() = parts.version;
        Ok(r)
    }

    match handler_impl(client, cfg, app.state(), req).await {
        Ok(response) => Ok(response.into_response()),
        Err(e) => {
            tracing::error!("{}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn ws(Extension(cfg): Extension<Arc<Cfg>>, Extension(app): Extension<Arc<App>>, ws: WebSocketUpgrade, req: Request) -> impl IntoResponse {
    ws.on_upgrade(move |ws| handle_socket(ws, cfg, app.state(), req))
}

async fn handle_socket(proxy_socket: WebSocket, cfg: Arc<Cfg>, state: Arc<State>, req: Request) {
    async fn handler_impl(proxy_socket: WebSocket, cfg: Arc<Cfg>, state: Arc<State>, req: Request) -> anyhow::Result<()> {
        let path = req.uri().path();
        let path_query = req
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or(path);

        let uri = format!("wss://{}{}", cfg.host, path_query);
        tracing::info!("WS {}", uri);
        
        let mut request = Request::builder()
            .uri(uri);
        
        let headers = request.headers_mut().context("No headers in request")?;
        for (key, value) in req.headers() {
            if key == http::header::HOST {
                headers.insert(key, cfg.host.parse()?);
            } else {
                headers.insert(key, value.to_owned());
            }
        }

        request.headers_mut().unwrap().insert("Sec-WebSocket-Key", generate_key().parse()?);
        
        let request = request.body(()).unwrap();

        let config = Arc::new(build_client_config());

        let (pa_ws_stream, _) = connect_async_tls_with_config(
            request,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(config))
        ).await?;

        let (mut pa_ws_writer, mut pa_ws_reader) =
            pa_ws_stream.split();

        let (mut proxy_ws_writer, mut proxy_ws_reader) =
            proxy_socket.split();

        let s = state.clone();
        tokio::spawn( async move {
            while let Some(msg) = pa_ws_reader.next().await {
                let msg = if let Ok(msg) = msg {
                    msg
                } else {
                    return;
                };

                s.update_ws_traffic(msg.len() as u64);

                let ws_msg = if let Some(msg) = tungstein_to_axum(msg) {
                    msg 
                } else {
                    continue
                };

                if let Err(e) = proxy_ws_writer.send(ws_msg).await {
                    tracing::info!("WebSocket error: {}", e);
                }
            }
        });

        while let Some(msg) = proxy_ws_reader.next().await {
            let msg = if let Ok(msg) = msg {
                msg
            } else {
                // client disconnected
                return Ok(());
            };

            let msg_size = match msg {
                axum::extract::ws::Message::Binary(ref bytes) => bytes.len(),
                axum::extract::ws::Message::Text(ref txt) => txt.as_bytes().len(),
                _ => 0
            };
            state.update_ws_traffic(msg_size as u64);

            pa_ws_writer.send(axum_to_tungstein(msg)).await?;
        }

        Ok(())
    }


    if let Err(e) = handler_impl(proxy_socket, cfg, state, req).await {
        tracing::error!("{}", e);
    };
}

fn get_static_serve_service(path: &String, sub_path: Option<&str>) -> ServeDir {
    let path = sub_path
        .map(|sub_path| [path, sub_path].iter().collect::<PathBuf>())
        .unwrap_or(path.into());

    ServeDir::new(path)
}

pub fn get_router(cfg: Arc<Cfg>, app: Arc<App>, transparent: bool) -> anyhow::Result<Router> {
    let client = build_https_client()?;
    if cfg.pagrid {
        Ok(get_pag_router(cfg, client, app, transparent))
    } else {
        Ok(get_pa6_router(cfg, client, app, transparent))
    }
}

macro_rules! help_path {
    ($prefix:expr, $path:expr) => {
        &($prefix.to_owned() + $path)
    };
}

fn get_pa6_help_subrouter(prefix: &str) -> Router {
    Router::new()
        .route(help_path!(prefix, "/search"), get(handler))
        .route(help_path!(prefix, "/searchprogress"), get(handler))
        .route(help_path!(prefix, "/context/node-view"), get(handler))
        .route(help_path!(prefix, "/context/node-wizard"), get(handler))
}

fn get_pa6_router(cfg: Arc<Cfg>, client: HTTPSClient, app: Arc<App>, transparent: bool) -> Router {
    let mut router = Router::new()
        .merge(get_pa6_help_subrouter("/polyanalyst/help"));
    
    if !transparent {
        router = router.nest_service(
            "/polyanalyst/static", 
            get_static_serve_service(&cfg.sourcedata, None)
        )
        .nest_service(
            "/polyanalyst/help", 
            get_static_serve_service(&cfg.help, None)
        );
    }
    
    router
        .route("/polyanalyst/eventsSocket", get(ws))
        .fallback(handler)
        .layer(Extension(client))
        .layer(Extension(cfg.clone()))
        .layer(Extension(app.clone()))
        .layer(TraceLayer::new_for_http())
}

fn get_pag_router(cfg: Arc<Cfg>, client: HTTPSClient, app: Arc<App>, transparent: bool) -> Router {
    let static_paths: Vec<(&str, Option<&str>)> = vec![
        ("/fonts", Some("fonts")),
        ("/vendor", Some("vendor")),
        ("/images", Some("images")),
        ("/scripts", Some("scripts")),
        ("/styles", Some("styles")),
        ("/localization", Some("localization")),
    ];

    let mut router = Router::new()
        .route("/ws", get(ws))
        .route("/api", get(handler).post(handler));

    if !transparent {
        for (route, sub_path) in static_paths {
            router = router.nest_service(route, get_static_serve_service(&cfg.sourcedata, sub_path));
        }
    }

    if !transparent {
        router = router
            .nest_service(
                "/help", 
                get_static_serve_service(&cfg.help, None)
            );
    }
    
    router
        .fallback(handler)
        .layer(Extension(client))
        .layer(Extension(cfg.clone()))
        .layer(Extension(app.clone()))
        .layer(TraceLayer::new_for_http())
}
