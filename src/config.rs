use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub struct Mapping {
    pub local_port: u16,
    pub target_address: String,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub mappings: Vec<Mapping>,
    pub transparent: bool,
    pub manage_iptables: bool,
    pub should_exit: bool,
}
#[async_trait]
pub trait ConfigProvider {
    async fn read_config(&self) -> anyhow::Result<Config>;
}

pub struct FileConfigProvider {
    pub config_path: PathBuf,
}

#[async_trait]
impl ConfigProvider for FileConfigProvider {
    async fn read_config(&self) -> anyhow::Result<Config> {
        let config_bytes = tokio::fs::read(&self.config_path).await?;
        let config: Config = serde_json::from_slice(&config_bytes)?;
        Ok(config)
    }
}
