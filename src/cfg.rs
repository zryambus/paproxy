use anyhow::Context;
use config::{Config, File, FileFormat, Source};
use serde_derive::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
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
    /// Proxy static routes instead of serving local static files, also set by --transparent
    #[serde(default)]
    pub transparent: bool,
    /// Accept any upstream TLS certificate, e.g. a self-signed one
    #[serde(default = "default_true")]
    pub insecure_upstream: bool,
    /// PEM file with extra CA certificates trusted for upstream, used only when insecure_upstream is false
    pub upstream_ca_path: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            port: 3000,
            https_port: None,
            certificate_path: None,
            key_path: None,
            sourcedata: None,
            help: None,
            host: String::new(),
            pagrid: false,
            log_headers: false,
            transparent: false,
            insecure_upstream: true,
            upstream_ca_path: None,
        }
    }
}

impl Cfg {
    /// Writes the settings into an existing YAML config: replaces the lines of known keys
    /// and appends the missing ones, comments and other lines are kept
    #[cfg(any(feature = "gui", test))]
    pub fn update_yaml(&self, existing: &str) -> String {
        let mut lines = self.yaml_lines();
        let mut yaml = String::new();

        for line in existing.lines() {
            let key = line.split_once(':').map(|(key, _)| key);
            match lines.iter().position(|(name, _)| Some(*name) == key) {
                Some(i) => yaml.push_str(&lines.remove(i).1),
                None => yaml.push_str(line),
            }
            yaml.push('\n');
        }
        for (_, line) in lines {
            yaml.push_str(&line);
            yaml.push('\n');
        }
        yaml
    }

    #[cfg(any(feature = "gui", test))]
    fn yaml_lines(&self) -> Vec<(&'static str, String)> {
        fn quoted(value: &str) -> String {
            format!("'{}'", value.replace('\'', "''"))
        }
        fn optional(value: &Option<String>) -> String {
            value.as_deref().map(quoted).unwrap_or_default()
        }

        [
            ("port", self.port.to_string()),
            (
                "https_port",
                self.https_port.map(|p| p.to_string()).unwrap_or_default(),
            ),
            ("sourcedata", optional(&self.sourcedata)),
            ("help", optional(&self.help)),
            ("host", quoted(&self.host)),
            ("pagrid", self.pagrid.to_string()),
            ("certificate_path", optional(&self.certificate_path)),
            ("key_path", optional(&self.key_path)),
            ("log_headers", self.log_headers.to_string()),
            ("transparent", self.transparent.to_string()),
            ("insecure_upstream", self.insecure_upstream.to_string()),
            ("upstream_ca_path", optional(&self.upstream_ca_path)),
        ]
        .into_iter()
        .map(|(key, value)| {
            let line = if value.is_empty() {
                format!("{}:", key)
            } else {
                format!("{}: {}", key, value)
            };
            (key, line)
        })
        .collect()
    }
}

pub fn get_config(source: Option<PathBuf>) -> anyhow::Result<Cfg> {
    let source = if let Some(source) = source {
        File::from(source)
    } else {
        File::new("config", FileFormat::Yaml)
    };
    build_config(source)
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
        assert!(!cfg.transparent);
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
    fn yaml_round_trips() {
        let cfg = Cfg {
            https_port: Some(3001),
            sourcedata: Some(r"D:\pa\it's here: #1".into()),
            host: "localhost:5043".into(),
            pagrid: true,
            transparent: true,
            insecure_upstream: false,
            ..Cfg::default()
        };

        let parsed = parse(&cfg.update_yaml("")).unwrap();

        assert_eq!(parsed.port, 3000);
        assert_eq!(parsed.https_port, Some(3001));
        assert_eq!(parsed.sourcedata, cfg.sourcedata);
        assert_eq!(parsed.help, None);
        assert_eq!(parsed.host, "localhost:5043");
        assert!(parsed.pagrid);
        assert_eq!(parsed.certificate_path, None);
        assert!(!parsed.log_headers);
        assert!(parsed.transparent);
        assert!(!parsed.insecure_upstream);
    }

    #[test]
    fn yaml_update_keeps_comments() {
        let existing = "# Local proxy\nport: 3000\n# host: old:5043\nhost: localhost:5043\n";
        let cfg = Cfg {
            port: 4000,
            host: "new:5043".into(),
            ..Cfg::default()
        };

        let yaml = cfg.update_yaml(existing);

        assert!(
            yaml.starts_with("# Local proxy\nport: 4000\n# host: old:5043\nhost: 'new:5043'\n")
        );
        assert!(yaml.ends_with("upstream_ca_path:\n"));
        assert_eq!(parse(&yaml).unwrap().port, 4000);
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
