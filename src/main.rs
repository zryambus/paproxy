mod cfg;
mod ws;
mod tls;
mod router;

use std::{net::SocketAddr};
use router::get_router;
use cfg::{get_config};

async fn main_impl() {
    let cfg = get_config();
    let router = get_router(cfg.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));
    axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .await
        .unwrap();
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(main_impl());
}

