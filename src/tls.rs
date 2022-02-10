use hyper::{Client, client::HttpConnector};
use hyper_tls::HttpsConnector;
use native_tls::TlsConnector;

pub type HTTPSClient = Client<HttpsConnector<HttpConnector>>;

pub fn build_tls_connector() -> anyhow::Result<TlsConnector> {
    Ok(
        native_tls::TlsConnector::builder()
            .danger_accept_invalid_hostnames(true)
            .danger_accept_invalid_certs(true)
            .build()?
    )
}

pub fn build_https_client() -> anyhow::Result<HTTPSClient> {
    let mut http = hyper::client::HttpConnector::new();
    http.enforce_http(false);

    let tls_connector = build_tls_connector()?;

    let https = HttpsConnector::from((http, tls_connector.into()));
    Ok(Client::builder().build::<_, hyper::Body>(https))
}
