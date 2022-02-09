
use std::sync::Arc;
use config::{Config};
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Cfg {
    pub port: u16,
    pub sourcedata: String,
    pub help: String,
    pub host: String,
}

pub fn get_config() -> Arc<Cfg> {
    let cfg: Cfg = Config::default()
        .with_merged(config::File::with_name("config.yml"))
        .unwrap()
        .try_into()
        .unwrap();

    cfg.into()
}
