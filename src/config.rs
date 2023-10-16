use async_trait::async_trait;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, PartialEq, Copy, Clone)]
pub struct IntMapping {
    pub local_bind: SocketAddrV4,
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
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub bind_addrs: Vec<String>,
}

impl Config {
    pub fn to_int_mappings(&self) -> Result<Vec<IntMapping>, anyhow::Error> {
        let mut result = vec![];
        for mapping in &self.mappings {
            let hairpin_net = match &mapping.hairpin_net {
                Some(hairpin_net) => Some(hairpin_net.parse::<Ipv4Net>()?),
                None => None,
            };
            for bind_addr in &self.bind_addrs {
                result.push(IntMapping {
                    local_bind: SocketAddrV4::new(
                        bind_addr.parse::<Ipv4Addr>()?,
                        mapping.local_port,
                    ),
                    target_address: mapping.target_address.parse::<SocketAddrV4>()?,
                    transparent: self.transparent,
                    manage_iptables: self.manage_iptables,
                    hairpin_net,
                });
            }
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
    pub bind_loopback: bool,
}

#[async_trait]
impl ConfigProvider for FileConfigProvider {
    async fn read_config(&self) -> anyhow::Result<(Config, Vec<IntMapping>)> {
        let config_bytes = tokio::fs::read(&self.config_path).await?;
        let mut config: Config = serde_json::from_slice(&config_bytes)?;
        if config.bind_addrs.is_empty() {
            config.bind_addrs = crate::bind_addr::get_bind_addresses(self.bind_loopback)
                .await?
                .iter()
                .map(|a| a.to_string())
                .collect();
        }
        config.bind_addrs.sort();

        let int_mappings = config.to_int_mappings()?;
        Ok((config, int_mappings))
    }
}
