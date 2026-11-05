use anyhow::Context;
use config::{Config, File, FileFormat, Source};
use serde_derive::Deserialize;
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Deserialize)]
pub struct Cfg {
    pub port: u16,
    /// Required only when both TLS paths are set
    pub https_port: Option<u16>,
    pub certificate_path: Option<String>,
    pub key_path: Option<String>,
    /// Not used in transparent mode
    pub sourcedata: Option<String>,
    /// Not used in transparent mode
    pub help: Option<String>,
    pub host: String,
    #[serde(default)]
    pub pagrid: bool,
    #[serde(default)]
    pub log_headers: bool,
    /// Accept any upstream TLS certificate, e.g. a self-signed one
    #[serde(default = "default_true")]
    pub insecure_upstream: bool,
    /// PEM file with extra CA certificates trusted for upstream, used only when insecure_upstream is false
    pub upstream_ca_path: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn get_config(source: Option<PathBuf>) -> anyhow::Result<Arc<Cfg>> {
    let source = if let Some(source) = source {
        File::from(source)
    } else {
        File::new("config", FileFormat::Yaml)
    };
    Ok(build_config(source)?.into())
}

fn build_config(source: impl Source + Send + Sync + 'static) -> anyhow::Result<Cfg> {
    Config::builder()
        .add_source(source)
        .build()
        .context("Failed to build config")?
        .try_deserialize()
        .context("Failed to parse config")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> anyhow::Result<Cfg> {
        build_config(File::from_str(yaml, FileFormat::Yaml))
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let cfg = parse("port: 3000\nhost: 192.168.1.10:5043").unwrap();

        assert_eq!(cfg.port, 3000);
        assert_eq!(cfg.host, "192.168.1.10:5043");
        assert_eq!(cfg.https_port, None);
        assert_eq!(cfg.certificate_path, None);
        assert_eq!(cfg.key_path, None);
        assert_eq!(cfg.sourcedata, None);
        assert_eq!(cfg.help, None);
        assert!(!cfg.pagrid);
        assert!(!cfg.log_headers);
        assert!(cfg.insecure_upstream);
        assert_eq!(cfg.upstream_ca_path, None);
    }

    #[test]
    fn upstream_verification_can_be_enabled() {
        let cfg = parse(
            "port: 3000\nhost: localhost:5043\ninsecure_upstream: false\nupstream_ca_path: ca.pem",
        )
        .unwrap();

        assert!(!cfg.insecure_upstream);
        assert_eq!(cfg.upstream_ca_path.as_deref(), Some("ca.pem"));
    }

    #[test]
    fn example_config_parses() {
        let cfg = parse(include_str!("../config.yml.example")).unwrap();

        assert_eq!(cfg.port, 3000);
        assert_eq!(cfg.https_port, Some(3001));
        assert_eq!(cfg.sourcedata.as_deref(), Some(r"D:\pa\SourceData\www"));
        assert_eq!(cfg.help.as_deref(), Some(r"D:\pa\SourceData\www\help"));
        // Empty TLS paths must disable HTTPS
        assert_eq!(cfg.certificate_path, None);
        assert_eq!(cfg.key_path, None);
        assert!(!cfg.pagrid);
        assert!(!cfg.log_headers);
        assert!(cfg.insecure_upstream);
        assert_eq!(cfg.upstream_ca_path, None);
    }

    #[test]
    fn missing_host_is_rejected() {
        assert!(parse("port: 3000").is_err());
    }

    #[test]
    fn out_of_range_port_is_rejected() {
        assert!(parse("port: 70000\nhost: localhost:5043").is_err());
    }
}
