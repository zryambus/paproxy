use std::{
    borrow::Cow,
    collections::HashMap,
    path::Path,
    sync::{Arc, atomic::AtomicU64},
};

use anyhow::Context;
use axum::{
    Router,
    extract::{Extension, Request, WebSocketUpgrade, ws::WebSocket},
    response::{IntoResponse, Response},
    routing::get,
};
use axum_extra::extract::CookieJar;
use futures::{SinkExt, StreamExt};
use http::{HeaderMap, HeaderName, Method, header};
use http_body_util::BodyExt;
use human_bytes::human_bytes;
use hyper::{StatusCode, Uri};
use tokio_tungstenite::{
    connect_async_tls_with_config, tungstenite::handshake::client::generate_key,
};
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::{
    HEADERS_TARGET,
    cfg::Cfg,
    state::State,
    tls::{HTTPSClient, build_client_config, build_https_client},
    ws::{axum_to_tungstenite, tungstenite_to_axum},
};

struct TrafficCounter {
    sent: u64,
    received: AtomicU64,
    method: Method,
    path: String,
    session_id: String,
}

impl TrafficCounter {
    pub fn new(sent: u64, method: Method, path: String, session_id: String) -> Self {
        Self {
            sent,
            received: AtomicU64::new(0),
            method,
            path,
            session_id,
        }
    }

    pub fn update_received(&self, count: u64) {
        self.received
            .fetch_add(count, std::sync::atomic::Ordering::AcqRel);
    }
}

impl Drop for TrafficCounter {
    fn drop(&mut self) {
        tracing::info!(
            "{} {} {} sent: {}, received: {}",
            self.session_id,
            self.method,
            self.path,
            human_bytes(self.sent as f64),
            human_bytes(self.received.load(std::sync::atomic::Ordering::Relaxed) as f64)
        );
    }
}

// Headers meaningful only for a single connection, must not be forwarded by proxies (RFC 9110, 7.6.1)
const HOP_BY_HOP_HEADERS: [HeaderName; 8] = [
    header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

fn remove_hop_by_hop_headers(headers: &mut HeaderMap) {
    // Headers listed in Connection are hop-by-hop as well
    let listed: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();

    for name in listed.iter().chain(HOP_BY_HOP_HEADERS.iter()) {
        headers.remove(name);
    }
}

const PA6_SESSION_ID_COOKIE_NAME: &str = "sid";
const PAG_SESSION_ID_COOKIE_NAME: &str = "SessionID";
const PA6_NIL_SESSION_ID: &str = "0000000000000000";
const PAG_NIL_SESSION_ID: &str = "00000000-0000-0000-0000-000000000000";

async fn handler(
    Extension(client): Extension<HTTPSClient>,
    Extension(cfg): Extension<Arc<Cfg>>,
    Extension(state): Extension<Arc<State>>,
    req: Request,
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
            .unwrap_or(&path)
            .to_owned();
        let method = req.method().clone();

        let uri = format!("https://{}{}", cfg.host, path_query);
        *req.uri_mut() = Uri::try_from(uri)?;

        let headers = req.headers_mut();
        if headers.contains_key(http::header::HOST) {
            headers.insert(http::header::HOST, cfg.host.parse()?);
        }
        remove_hop_by_hop_headers(headers);

        if cfg.log_headers {
            let headers_map: HashMap<&str, Cow<str>> = headers
                .iter()
                .map(|(k, v)| (k.as_str(), String::from_utf8_lossy(v.as_bytes())))
                .collect();
            tracing::info!(target: HEADERS_TARGET, path = path_query, direction = "request", headers = ?headers_map);
        }

        let content_length;
        {
            let headers = req.headers_mut();
            content_length = headers
                .get(header::CONTENT_LENGTH)
                .and_then(|hv| hv.to_str().ok())
                .and_then(|str| str.parse::<u64>().ok());

            if let Some(content_length) = content_length {
                state.update_sent(&path, content_length);
            };
        }

        let response = client.request(req).await?;

        let (mut parts, incoming) = response.into_parts();
        remove_hop_by_hop_headers(&mut parts.headers);

        let session_id = parts
            .headers
            .get_all(http::header::SET_COOKIE)
            .iter()
            .find_map(|header| {
                if let Ok(header_str) = header.to_str() {
                    if let Ok(cookie) = cookie::Cookie::parse(header_str) {
                        let target_name = if cfg.pagrid {
                            PAG_SESSION_ID_COOKIE_NAME
                        } else {
                            PA6_SESSION_ID_COOKIE_NAME
                        };
                        if cookie.name() == target_name {
                            return Some(cookie.value().to_owned());
                        }
                    }
                    return None;
                }
                None
            });

        let counter = TrafficCounter::new(
            content_length.unwrap_or_default(),
            method,
            path.clone(),
            session_id.unwrap_or(if cfg.pagrid {
                PAG_NIL_SESSION_ID.into()
            } else {
                PA6_NIL_SESSION_ID.into()
            }),
        );

        let inspected_incoming = incoming.map_frame(move |frame| {
            if let Some(chunk) = frame.data_ref() {
                let len = chunk.len();
                state.update_received(&path, len as u64);
                counter.update_received(len as u64);
            }
            frame
        });

        if cfg.log_headers {
            tracing::info!(target: HEADERS_TARGET, path = path_query, direction = "response", headers = ?parts.headers);
        }

        let mut r = Response::new(inspected_incoming).into_response();
        *r.headers_mut() = parts.headers;
        *r.status_mut() = parts.status;
        *r.version_mut() = parts.version;

        Ok(r)
    }

    match handler_impl(client, cfg, state, req).await {
        Ok(response) => Ok(response.into_response()),
        Err(e) => {
            tracing::error!("{}", e);
            // Failure to reach upstream is not the proxy's own fault
            if e.downcast_ref::<hyper_util::client::legacy::Error>()
                .is_some()
            {
                Err(StatusCode::BAD_GATEWAY)
            } else {
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

async fn ws(
    Extension(cfg): Extension<Arc<Cfg>>,
    Extension(state): Extension<Arc<State>>,
    ws: WebSocketUpgrade,
    req: Request,
) -> impl IntoResponse {
    ws.on_upgrade(move |ws| handle_socket(ws, cfg, state, req))
}

async fn handle_socket(proxy_socket: WebSocket, cfg: Arc<Cfg>, state: Arc<State>, req: Request) {
    async fn handler_impl(
        proxy_socket: WebSocket,
        cfg: Arc<Cfg>,
        state: Arc<State>,
        req: Request,
    ) -> anyhow::Result<()> {
        let path = req.uri().path();
        let path_query = req
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or(path);

        let jar = CookieJar::from_headers(req.headers());
        let session_id = jar
            .get(if cfg.pagrid {
                PAG_SESSION_ID_COOKIE_NAME
            } else {
                PA6_SESSION_ID_COOKIE_NAME
            })
            .map(|c| c.value())
            .unwrap_or(if cfg.pagrid {
                PAG_NIL_SESSION_ID
            } else {
                PA6_NIL_SESSION_ID
            });

        let uri = format!("wss://{}{}", cfg.host, path_query);
        tracing::info!("{} WS {}", session_id, path_query);

        let mut request = Request::builder().uri(uri);

        let headers = request.headers_mut().context("No headers in request")?;
        for (key, value) in req.headers() {
            if key == http::header::HOST {
                headers.insert(key, cfg.host.parse()?);
            } else {
                headers.insert(key, value.to_owned());
            }
        }

        request
            .headers_mut()
            .unwrap()
            .insert("Sec-WebSocket-Key", generate_key().parse()?);

        let request = request.body(()).unwrap();

        let config = Arc::new(build_client_config());

        let (pa_ws_stream, _) = connect_async_tls_with_config(
            request,
            None,
            false,
            Some(tokio_tungstenite::Connector::Rustls(config)),
        )
        .await?;

        let (mut pa_ws_writer, mut pa_ws_reader) = pa_ws_stream.split();

        let (mut proxy_ws_writer, mut proxy_ws_reader) = proxy_socket.split();

        let upstream_to_client = async {
            while let Some(msg) = pa_ws_reader.next().await {
                let Ok(msg) = msg else {
                    // upstream disconnected
                    return;
                };

                state.update_ws_traffic(msg.len() as u64);

                let Some(ws_msg) = tungstenite_to_axum(msg) else {
                    continue;
                };

                if let Err(e) = proxy_ws_writer.send(ws_msg).await {
                    tracing::info!("WebSocket error: {}", e);
                    return;
                }
            }
        };

        let client_to_upstream = async {
            while let Some(msg) = proxy_ws_reader.next().await {
                let Ok(msg) = msg else {
                    // client disconnected
                    return Ok(());
                };

                let msg_size = match msg {
                    axum::extract::ws::Message::Binary(ref bytes) => bytes.len(),
                    axum::extract::ws::Message::Text(ref txt) => txt.len(),
                    _ => 0,
                };
                state.update_ws_traffic(msg_size as u64);

                pa_ws_writer.send(axum_to_tungstenite(msg)).await?;
            }
            anyhow::Ok(())
        };

        // As soon as either side is done, both connections are dropped
        tokio::select! {
            _ = upstream_to_client => Ok(()),
            result = client_to_upstream => result,
        }
    }

    if let Err(e) = handler_impl(proxy_socket, cfg, state, req).await {
        tracing::error!("{}", e);
    };
}

fn get_static_root<'a>(path: &'a Option<String>, name: &str) -> anyhow::Result<&'a Path> {
    path.as_deref()
        .map(Path::new)
        .with_context(|| format!("{} must be set unless --transparent is used", name))
}

fn get_static_serve_service(path: &Path, sub_path: Option<&str>) -> ServeDir {
    let path = sub_path
        .map(|sub_path| path.join(sub_path))
        .unwrap_or(path.into());

    // ServeDir silently responds with 404 for a missing directory
    if !path.is_dir() {
        tracing::warn!("Static files directory does not exist: {}", path.display());
    }

    ServeDir::new(path)
}

pub fn get_router(cfg: Arc<Cfg>, state: Arc<State>, transparent: bool) -> anyhow::Result<Router> {
    let client = build_https_client()?;
    if cfg.pagrid {
        get_pag_router(cfg, client, state, transparent)
    } else {
        get_pa6_router(cfg, client, state, transparent)
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

fn get_pa6_router(
    cfg: Arc<Cfg>,
    client: HTTPSClient,
    state: Arc<State>,
    transparent: bool,
) -> anyhow::Result<Router> {
    let mut router = Router::new().merge(get_pa6_help_subrouter("/polyanalyst/help"));

    if !transparent {
        let sourcedata = get_static_root(&cfg.sourcedata, "sourcedata")?;
        let help = get_static_root(&cfg.help, "help")?;
        router = router
            .nest_service(
                "/polyanalyst/static",
                get_static_serve_service(sourcedata, None),
            )
            .nest_service("/polyanalyst/help", get_static_serve_service(help, None));
    }

    Ok(router
        .route("/polyanalyst/eventsSocket", get(ws))
        .fallback(handler)
        .layer(Extension(client))
        .layer(Extension(cfg.clone()))
        .layer(Extension(state.clone()))
        .layer(TraceLayer::new_for_http()))
}

fn get_pag_router(
    cfg: Arc<Cfg>,
    client: HTTPSClient,
    state: Arc<State>,
    transparent: bool,
) -> anyhow::Result<Router> {
    let static_paths: Vec<(&str, Option<&str>)> = vec![
        ("/fonts", Some("fonts")),
        ("/vendor", Some("vendor")),
        ("/images", Some("images")),
        ("/scripts", Some("scripts")),
        ("/styles", Some("styles")),
        ("/localization", Some("localization")),
    ];

    let mut router = Router::new().route("/ws", get(ws));

    if !transparent {
        let sourcedata = get_static_root(&cfg.sourcedata, "sourcedata")?;
        let help = get_static_root(&cfg.help, "help")?;
        for (route, sub_path) in static_paths {
            router = router.nest_service(route, get_static_serve_service(sourcedata, sub_path));
        }
        router = router.nest_service("/help", get_static_serve_service(help, None));
    }

    Ok(router
        .fallback(handler)
        .layer(Extension(client))
        .layer(Extension(cfg.clone()))
        .layer(Extension(state.clone()))
        .layer(TraceLayer::new_for_http()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;

    use super::*;

    // Nothing listens on port 1, so every proxied request fails to connect
    const UNREACHABLE_HOST: &str = "127.0.0.1:1";

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("paproxy-{}-{}", name, std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, relative: &str, content: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }

        fn path(&self) -> Option<String> {
            Some(self.0.to_string_lossy().into_owned())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn cfg(pagrid: bool, sourcedata: Option<String>, help: Option<String>) -> Arc<Cfg> {
        Arc::new(Cfg {
            port: 0,
            https_port: None,
            certificate_path: None,
            key_path: None,
            sourcedata,
            help,
            host: UNREACHABLE_HOST.into(),
            pagrid,
            log_headers: false,
        })
    }

    fn router(cfg: Arc<Cfg>, transparent: bool) -> Router {
        get_router(cfg, Arc::new(State::new()), transparent).unwrap()
    }

    async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[test]
    fn hop_by_hop_headers_are_removed() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONNECTION,
            "keep-alive, X-Custom-Hop".parse().unwrap(),
        );
        headers.insert("keep-alive", "timeout=5".parse().unwrap());
        headers.insert("x-custom-hop", "1".parse().unwrap());
        headers.insert(header::TRANSFER_ENCODING, "chunked".parse().unwrap());
        headers.insert(header::PROXY_AUTHORIZATION, "Basic abc".parse().unwrap());
        headers.insert(header::COOKIE, "sid=1".parse().unwrap());
        headers.insert(header::CONTENT_TYPE, "text/html".parse().unwrap());

        remove_hop_by_hop_headers(&mut headers);

        let mut names: Vec<&str> = headers.keys().map(|name| name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["content-type", "cookie"]);
    }

    #[tokio::test]
    async fn pa6_serves_local_static_files() {
        let sourcedata = TempDir::new("pa6-static");
        sourcedata.file("app.js", "app");
        let help = TempDir::new("pa6-help");
        help.file("index.html", "help");
        let router = router(cfg(false, sourcedata.path(), help.path()), false);

        assert_eq!(
            get(&router, "/polyanalyst/static/app.js").await,
            (StatusCode::OK, "app".into())
        );
        assert_eq!(
            get(&router, "/polyanalyst/help/index.html").await,
            (StatusCode::OK, "help".into())
        );
    }

    #[tokio::test]
    async fn pa6_help_search_is_proxied_not_served_locally() {
        let sourcedata = TempDir::new("pa6-search-static");
        let help = TempDir::new("pa6-search-help");
        help.file("search", "local");
        let router = router(cfg(false, sourcedata.path(), help.path()), false);

        let (status, _) = get(&router, "/polyanalyst/help/search").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn pag_serves_local_static_files() {
        let sourcedata = TempDir::new("pag-static");
        sourcedata.file("scripts/app.js", "app");
        let help = TempDir::new("pag-help");
        help.file("index.html", "help");
        let router = router(cfg(true, sourcedata.path(), help.path()), false);

        assert_eq!(
            get(&router, "/scripts/app.js").await,
            (StatusCode::OK, "app".into())
        );
        assert_eq!(
            get(&router, "/help/index.html").await,
            (StatusCode::OK, "help".into())
        );
    }

    #[tokio::test]
    async fn static_dirs_are_required_without_transparent_mode() {
        for pagrid in [false, true] {
            let result = get_router(cfg(pagrid, None, None), Arc::new(State::new()), false);
            assert!(result.is_err(), "pagrid: {}", pagrid);
        }
    }

    #[tokio::test]
    async fn transparent_mode_proxies_static_routes() {
        let router = router(cfg(false, None, None), true);

        let (status, _) = get(&router, "/polyanalyst/static/app.js").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn unreachable_upstream_returns_bad_gateway() {
        for pagrid in [false, true] {
            let router = router(cfg(pagrid, None, None), true);

            let (status, _) = get(&router, "/api").await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "pagrid: {}", pagrid);
        }
    }
}
