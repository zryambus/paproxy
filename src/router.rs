use std::{sync::Arc, path::PathBuf};

use anyhow::Context;
use axum::{
    Router,
    routing::{get, MethodRouter, get_service},
    extract::{ws::WebSocket, WebSocketUpgrade, Extension},
    response::{IntoResponse, Response}
};
use futures_util::{StreamExt, SinkExt};
use hyper::{StatusCode, Uri, Request, Body};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::handshake::client::generate_key};
use tower_http::services::ServeDir;
use tracing;

use crate::{
    cfg::Cfg,
    tls::{HTTPSClient, build_tls_connector, build_https_client},
    ws::{axum_to_tungstein, tungstein_to_axum},
};

async fn handler(
    Extension(client): Extension<HTTPSClient>,
    Extension(cfg): Extension<Arc<Cfg>>,
    req: Request<Body>
) -> std::result::Result<Response<Body>, StatusCode> {
    async fn handler_impl(
        client: HTTPSClient,
        cfg: Arc<Cfg>,
        mut req: Request<Body>
    ) -> anyhow::Result<Response<Body>> {
        let path = req.uri().path();
        let path_query = req
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or(path);
    
        let uri = format!("https://{}{}", cfg.host, path_query);
        tracing::info!("{} {}", req.method(), uri);

        *req.uri_mut() = Uri::try_from(uri)?;

        let headers = req.headers_mut();
        if headers.contains_key(http::header::HOST) {
            headers.insert(http::header::HOST, cfg.host.parse()?);
        }
    
        let response = client.request(req).await?;

        Ok(response)
    }

    match handler_impl(client, cfg, req).await {
        Ok(response) => Ok(response),
        Err(e) => {
            tracing::error!("{}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn ws(Extension(cfg): Extension<Arc<Cfg>>, ws: WebSocketUpgrade, req: Request<Body>) -> impl IntoResponse {
    ws.on_upgrade(|ws| handle_socket(ws, cfg, req))
}

async fn handle_socket(proxy_socket: WebSocket, cfg: Arc<Cfg>, req: Request<Body>) {
    async fn handler_impl(proxy_socket: WebSocket, cfg: Arc<Cfg>, req: Request<Body>) -> anyhow::Result<()> {
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
            
            let headers = request.headers_mut().context("No headers in hrequest")?;
            for (key, value) in req.headers() {
                if key == http::header::HOST {
                    headers.insert(key, cfg.host.parse()?);
                } else {
                    headers.insert(key, value.to_owned());
                }
            }

            request.headers_mut().unwrap().insert("Sec-WebSocket-Key", generate_key().parse()?);
            
            let request = request.body(()).unwrap();

        let tls_connector = build_tls_connector()?;

        let (pa_ws_stream, _) = connect_async_tls_with_config(
            request,
            None,
            Some(tokio_tungstenite::Connector::NativeTls(tls_connector))
        ).await?;
        
        let (mut pa_ws_writer, mut pa_ws_reader) =
            pa_ws_stream.split();

        let (mut proxy_ws_writer, mut proxy_ws_reader) =
            proxy_socket.split();

        tokio::spawn( async move {
            while let Some(msg) = pa_ws_reader.next().await {
                let msg = if let Ok(msg) = msg {
                    msg
                } else {
                    return;
                };

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

            pa_ws_writer.send(axum_to_tungstein(msg)).await?;
        }

        Ok(())
    }


    if let Err(e) = handler_impl(proxy_socket, cfg, req).await {
        tracing::error!("{}", e);
    };
}

fn get_static_serve_service(path: &String, sub_path: Option<&str>) -> MethodRouter {
    let path = if let Some(sub_path) = sub_path {
        let mut tmp = PathBuf::from(path);
        tmp.push(sub_path);
        tmp
    } else {
        path.into()
    };

    get_service(ServeDir::new(path))
        .handle_error(|error: std::io::Error| async move {
            tracing::error!("{}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", error),
            )
        })
}

pub fn get_router(cfg: Arc<Cfg>) -> anyhow::Result<Router> {
    let client = build_https_client()?;
    if cfg.pagrid {
        Ok(get_pag_router(cfg, client))
    } else {
        Ok(get_pa6_router(cfg, client))
    }

}

fn get_pa6_router(cfg: Arc<Cfg>, client: HTTPSClient) -> Router {
    Router::new()
        .nest(
            "/polyanalyst/static", 
            get_static_serve_service(&cfg.sourcedata, None)
        )
        .nest(
            "/polyanalyst/help", 
            get_static_serve_service(&cfg.help, None)
        )
        .route("/polyanalyst/eventsSocket", get(ws))
        .fallback(get(handler).post(handler))
        .layer(Extension(client))
        .layer(Extension(cfg.clone()))
}

fn get_pag_router(cfg: Arc<Cfg>, client: HTTPSClient) -> Router {
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

    for (route, sub_path) in static_paths {
        router = router.nest(route, get_static_serve_service(&cfg.sourcedata, sub_path));
    }

    router
        .nest(
            "/help", 
            get_static_serve_service(&cfg.help, None)
        )
        .fallback(get(handler).post(handler))
        .layer(Extension(client))
        .layer(Extension(cfg.clone()))
}