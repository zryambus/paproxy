
use std::sync::Arc;
use config::{Config, FileFormat, File};
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Cfg {
    pub port: u16,
    pub sourcedata: String,
    pub help: String,
    pub host: String,
}

pub fn get_config() -> anyhow::Result<Arc<Cfg>> {
    let cfg: Cfg = Config::builder()
        .add_source(File::new("config", FileFormat::Yaml))
        .build()?
        .try_deserialize()?;

    Ok(cfg.into())
}
