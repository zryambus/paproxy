use std::sync::Arc;

use anyhow::Context;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::{
    DigitallySignedStruct, RootCertStore,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject},
};

use crate::cfg::Cfg;

pub type HTTPSClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, axum::body::Body>;

#[derive(Debug)]
struct DummyVerifier {}

impl ServerCertVerifier for DummyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}

/// TLS settings for connections to upstream, shared by HTTP and WebSocket proxying
pub fn build_client_config(cfg: &Cfg) -> anyhow::Result<rustls::ClientConfig> {
    if cfg.insecure_upstream {
        if cfg.upstream_ca_path.is_some() {
            tracing::warn!("upstream_ca_path is ignored because insecure_upstream is enabled");
        }
        tracing::info!("Upstream TLS certificate verification is disabled");

        let verifier = Arc::new(DummyVerifier {});
        return Ok(rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth());
    }

    let mut roots = RootCertStore::empty();

    let native = rustls_native_certs::load_native_certs();
    for e in native.errors {
        tracing::warn!("Failed to load system CA certificates: {}", e);
    }
    roots.add_parsable_certificates(native.certs);

    if let Some(path) = &cfg.upstream_ca_path {
        let certs = CertificateDer::pem_file_iter(path)
            .with_context(|| format!("Failed to read upstream CA file {}", path))?;
        for cert in certs {
            let cert = cert.with_context(|| format!("Bad upstream CA file {}", path))?;
            roots
                .add(cert)
                .with_context(|| format!("Invalid certificate in upstream CA file {}", path))?;
        }
    }

    if roots.is_empty() {
        anyhow::bail!("No trusted CA certificates found to verify upstream");
    }

    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

pub fn build_https_client(config: rustls::ClientConfig) -> HTTPSClient {
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(connector)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(insecure_upstream: bool, upstream_ca_path: Option<&str>) -> Cfg {
        Cfg {
            port: 0,
            https_port: None,
            certificate_path: None,
            key_path: None,
            sourcedata: None,
            help: None,
            host: "localhost:5043".into(),
            pagrid: false,
            log_headers: false,
            insecure_upstream,
            upstream_ca_path: upstream_ca_path.map(Into::into),
        }
    }

    #[test]
    fn insecure_upstream_builds() {
        assert!(build_client_config(&cfg(true, None)).is_ok());
    }

    #[test]
    fn verified_upstream_uses_system_roots() {
        assert!(build_client_config(&cfg(false, None)).is_ok());
    }

    #[test]
    fn missing_upstream_ca_file_is_rejected() {
        assert!(build_client_config(&cfg(false, Some("does-not-exist.pem"))).is_err());
    }
}
