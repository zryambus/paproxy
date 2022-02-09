use std::sync::Arc;

use axum::{
    Router, AddExtensionLayer,
    routing::{get, MethodRouter, get_service},
    extract::{ws::WebSocket, WebSocketUpgrade, Extension},
    response::{IntoResponse, Response}
};
use futures_util::{StreamExt, SinkExt};
use hyper::{StatusCode, Uri, Request, Body};
use tokio_tungstenite::connect_async_tls_with_config;
use tower_http::services::ServeDir;

use crate::{
    cfg::Cfg,
    tls::{HTTPSClient, build_tls_connector, build_https_client},
    ws::{axum_to_tungstein, tungstein_to_axum}
};

async fn handler(
    Extension(client): Extension<HTTPSClient>,
    Extension(cfg): Extension<Arc<Cfg>>,
    mut req: Request<Body>
) -> Response<Body> {
    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(path);

    let uri = format!("https://{}{}", cfg.host, path_query);

    *req.uri_mut() = Uri::try_from(uri).unwrap();

    client.request(req).await.unwrap()
}

async fn ws(Extension(cfg): Extension<Arc<Cfg>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|ws| handle_socket(ws, cfg))
}

async fn handle_socket(proxy_socket: WebSocket, cfg: Arc<Cfg>) {
    let uri = url::Url::parse(&format!("wss://{}/polyanalyst/eventsSocket", cfg.host)).unwrap();

    let tls_connector = build_tls_connector();

    let (pa_ws_stream, _) = connect_async_tls_with_config(
        uri,
        None,
        Some(tokio_tungstenite::Connector::NativeTls(tls_connector))
    ).await.unwrap();
    
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

            proxy_ws_writer.send(tungstein_to_axum(msg)).await.unwrap();
        }
    });

    while let Some(msg) = proxy_ws_reader.next().await {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            // client disconnected
            return;
        };

        pa_ws_writer.send(axum_to_tungstein(msg)).await.unwrap();
    }
}

fn get_static_serve_service(path: &String) -> MethodRouter {
    get_service(ServeDir::new(path))
        .handle_error(|error: std::io::Error| async move {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", error),
            )
        })
}

pub fn get_router(cfg: Arc<Cfg>) -> Router {
    let client = build_https_client();
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
        .layer(AddExtensionLayer::new(client))
        .layer(AddExtensionLayer::new(cfg.clone()))
}