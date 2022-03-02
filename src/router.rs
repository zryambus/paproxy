use std::{sync::Arc};

use axum::{
    Router,
    routing::{get, MethodRouter, get_service},
    extract::{ws::WebSocket, WebSocketUpgrade, Extension},
    response::{IntoResponse, Response}
};
use futures_util::{StreamExt, SinkExt};
use hyper::{StatusCode, Uri, Request, Body};
use tokio_tungstenite::connect_async_tls_with_config;
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

async fn ws(Extension(cfg): Extension<Arc<Cfg>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|ws| handle_socket(ws, cfg))
}

async fn handle_socket(proxy_socket: WebSocket, cfg: Arc<Cfg>) {
    async fn handler_impl(proxy_socket: WebSocket, cfg: Arc<Cfg>) -> anyhow::Result<()> {
        let uri = url::Url::parse(&format!("wss://{}/polyanalyst/eventsSocket", cfg.host))?;

        let tls_connector = build_tls_connector()?;

        let (pa_ws_stream, _) = connect_async_tls_with_config(
            uri,
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


    if let Err(e) = handler_impl(proxy_socket, cfg).await {
        tracing::error!("{}", e);
    };
}

fn get_static_serve_service(path: &String) -> MethodRouter {
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
    Ok(
        Router::new()
            .nest(
                "/polyanalyst/static", 
                get_static_serve_service(&cfg.sourcedata)
            )
            .nest(
                "/polyanalyst/help", 
                get_static_serve_service(&cfg.help)
            )
            .route("/polyanalyst/eventsSocket", get(ws))
            .fallback(get(handler).post(handler))
            .layer(Extension(client))
            .layer(Extension(cfg.clone()))
    )
}