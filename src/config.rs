use async_trait::async_trait;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, PartialEq, Copy, Clone)]
pub struct IntMapping {
    pub local_port: u16,
    pub target_address: SocketAddrV4,
    pub transparent: bool,
    pub manage_iptables: bool,
    pub hairpin_net: Option<Ipv4Net>,
}

impl IntMapping {
    pub(crate) fn connection_is_hairpin(&self, p0: &Ipv4Addr) -> bool {
        match self.hairpin_net {
            None => false,
            Some(net) => net.contains(p0),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub struct Mapping {
    pub local_port: u16,
    pub target_address: String,
    pub hairpin_net: Option<String>,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub mappings: Vec<Mapping>,
    pub transparent: bool,
    pub manage_iptables: bool,
    pub should_exit: bool,
}

impl Config {
    pub fn to_int_mappings(&self) -> Result<Vec<IntMapping>, anyhow::Error> {
        let mut result = vec![];
        for mapping in &self.mappings {
            let hairpin_net = match &mapping.hairpin_net {
                Some(hairpin_net) => Some(hairpin_net.parse::<Ipv4Net>()?),
                None => None,
            };
            result.push(IntMapping {
                local_port: mapping.local_port,
                target_address: mapping.target_address.parse::<SocketAddrV4>()?,
                transparent: self.transparent,
                manage_iptables: self.manage_iptables,
                hairpin_net,
            });
        }
        Ok(result)
    }
}

#[async_trait]
pub trait ConfigProvider {
    async fn read_config(&self) -> anyhow::Result<(Config, Vec<IntMapping>)>;
}

pub struct FileConfigProvider {
    pub config_path: PathBuf,
}

#[async_trait]
impl ConfigProvider for FileConfigProvider {
    async fn read_config(&self) -> anyhow::Result<(Config, Vec<IntMapping>)> {
        let config_bytes = tokio::fs::read(&self.config_path).await?;
        let config: Config = serde_json::from_slice(&config_bytes)?;
        let int_mappings = config.to_int_mappings()?;
        Ok((config, int_mappings))
    }
}
