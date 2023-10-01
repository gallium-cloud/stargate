use crate::config::ConfigProvider;
use crate::{async_proxy, iptables_setup};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddrV4;

use std::time::Duration;
use tokio::task::JoinHandle;

struct RunningProxy {
    handle: JoinHandle<anyhow::Result<()>>,
    target: SocketAddrV4,
}
pub async fn start(config_provider: impl ConfigProvider) {
    let mut config = config_provider.read_config().await.unwrap();

    if config.transparent && config.manage_iptables {
        iptables_setup::initial_setup().await.unwrap();
    }

    let mut proxies = HashMap::new();

    loop {
        let mut to_delete: HashSet<u16> = HashSet::from_iter(proxies.keys().cloned());

        if !config.should_exit {
            for mapping in &config.mappings {
                to_delete.remove(&mapping.local_port);
                if !proxies.contains_key(&mapping.local_port) {
                    if let Ok(target) = mapping.target_address.parse::<SocketAddrV4>() {
                        if config.transparent && config.manage_iptables {
                            iptables_setup::add_iptables_return_rule(target).await.ok();
                        }
                        let handle = tokio::spawn(async_proxy::start_proxy(
                            mapping.local_port.clone(),
                            target.clone().into(),
                            config.transparent,
                        ));
                        proxies.insert(mapping.local_port.clone(), RunningProxy { handle, target });
                    }
                }
            }
        }
        for port in to_delete {
            if let Some(instance) = proxies.remove(&port) {
                tracing::info!("dropping task for {port}");
                instance.handle.abort_handle().abort();
                iptables_setup::del_iptables_return_rule(instance.target)
                    .await
                    .ok();
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        if let Ok(new_config) = config_provider.read_config().await {
            if config != new_config {
                tracing::info!("new config detected");
                config = new_config;
            }
        }
    }
}
