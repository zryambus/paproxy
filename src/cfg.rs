
use std::{sync::Arc, path::PathBuf};
use config::{Config, FileFormat, File};
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Cfg {
    pub port: u16,
    pub sourcedata: String,
    pub help: String,
    pub host: String,
    pub pagrid: bool,
}

pub fn get_config(source: Option<PathBuf>) -> anyhow::Result<Arc<Cfg>> {
    let source = if let Some(source) = source {
        File::from(source)
    } else {
        File::new("config", FileFormat::Yaml)
    };
    let cfg: Cfg = Config::builder()
        .add_source(source)
        .build()?
        .try_deserialize()?;

    Ok(cfg.into())
}
