use std::sync::Arc;

use hyper::{Client, client::HttpConnector};
use rustls::client::ServerCertVerifier;

pub type HTTPSClient = Client<hyper_rustls::HttpsConnector<HttpConnector>>;

struct DummyVerifier{}

impl ServerCertVerifier for DummyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::Certificate,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::Certificate,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::HandshakeSignatureValid::assertion())
    }
}

pub fn build_client_config() -> rustls::ClientConfig {
    let verifier = Arc::new(DummyVerifier{});
    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config
}

pub fn build_tls_connector() -> anyhow::Result<hyper_rustls::HttpsConnector<HttpConnector>> {
    let config = build_client_config();
    Ok(
        hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .build()
    )
}

pub fn build_https_client() -> anyhow::Result<HTTPSClient> {
    let connector = build_tls_connector()?;
    let client: Client<_, hyper::Body> = Client::builder().build(connector);
    Ok(client)
}
