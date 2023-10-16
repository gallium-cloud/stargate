use crate::config::ConfigProvider;
use crate::{async_proxy, iptables_setup};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddrV4;

use std::time::Duration;
use tokio::task::JoinHandle;

struct RunningProxy {
    handle: JoinHandle<anyhow::Result<()>>,
    target: SocketAddrV4,
    managed: bool,
    cancel: tokio_util::sync::CancellationToken,
}
pub async fn start(config_provider: impl ConfigProvider) {
    let mut termination_tasks = tokio::task::JoinSet::new();
    let (mut config, mut int_mappings) = config_provider.read_config().await.unwrap();

    if config.transparent && config.manage_iptables {
        iptables_setup::initial_setup().await.unwrap();
    }

    let mut proxies: HashMap<SocketAddrV4, RunningProxy> = HashMap::new();

    loop {
        let mut to_delete: HashSet<SocketAddrV4> = HashSet::from_iter(proxies.keys().cloned());

        if !config.should_exit {
            for mapping in &int_mappings {
                to_delete.remove(&mapping.local_bind);
                if !proxies.contains_key(&mapping.local_bind) {
                    if config.transparent && config.manage_iptables {
                        iptables_setup::add_iptables_return_rule(mapping.target_address)
                            .await
                            .ok();
                    }
                    let cancel = tokio_util::sync::CancellationToken::new();
                    let handle =
                        tokio::spawn(async_proxy::start_proxy(mapping.clone(), cancel.clone()));
                    proxies.insert(
                        mapping.local_bind,
                        RunningProxy {
                            handle,
                            target: mapping.target_address,
                            managed: config.transparent && config.manage_iptables,
                            cancel,
                        },
                    );
                }
            }
        }
        for port in to_delete {
            if let Some(instance) = proxies.remove(&port) {
                tracing::info!("dropping task for {port}");
                termination_tasks.spawn(async move {
                    instance.cancel.cancel();
                    let _ = instance.handle.await.unwrap();
                    if instance.managed {
                        iptables_setup::del_iptables_return_rule(instance.target)
                            .await
                            .ok();
                    }
                });
            }
        }
        if config.should_exit && proxies.is_empty() {
            while !termination_tasks.is_empty() {
                termination_tasks.join_next().await;
            }
            return;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        if let Ok((new_config, new_mappings)) = config_provider.read_config().await {
            if config != new_config {
                tracing::info!("new config detected");
                config = new_config;
                int_mappings = new_mappings;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    pub struct TestTrivialConfigProvider {
        local_port: u16,
        target_address: String,
        should_exit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl ConfigProvider for TestTrivialConfigProvider {
        async fn read_config(
            &self,
        ) -> anyhow::Result<(crate::config::Config, Vec<crate::config::IntMapping>)> {
            let bind_addrs = crate::bind_addr::get_bind_addresses(true)
                .await
                .unwrap()
                .iter()
                .map(|a| a.to_string())
                .collect();
            let config = crate::config::Config {
                mappings: vec![crate::config::Mapping {
                    local_port: self.local_port,
                    target_address: self.target_address.clone(),
                    hairpin_net: None,
                }],
                transparent: false,
                manage_iptables: false,
                should_exit: self.should_exit.load(std::sync::atomic::Ordering::Relaxed),
                bind_addrs,
            };
            let int_mappings = config.to_int_mappings()?;
            Ok((config, int_mappings))
        }
    }

    #[tokio::test]
    async fn smoke() {
        let target_port = portpicker::pick_unused_port().unwrap();
        let target = tokio::spawn(async move {
            let socket = tokio::net::TcpListener::bind(format!("127.0.0.1:{target_port}"))
                .await
                .unwrap();
            while let Ok((stream, _)) = socket.accept().await {
                let (stream_in, mut stream_out) = stream.into_split();
                let mut stream_in = tokio::io::BufReader::new(stream_in);
                let mut line = String::new();
                while let Ok(n) = stream_in.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }
                    stream_out.write_all(line.as_bytes()).await.unwrap();
                    line = String::new();
                }
            }
        });

        let _ = wait_for_them::wait_for_them(
            &[wait_for_them::ToCheck::HostnameAndPort(
                "127.0.0.1".to_string(),
                target_port,
            )],
            Some(1000),
            None,
            true,
        )
        .await
        .iter()
        .map(|o| o.expect("target unreachable"))
        .collect::<Vec<_>>();

        let local_port = portpicker::pick_unused_port().unwrap();
        let should_exit: std::sync::Arc<std::sync::atomic::AtomicBool> =
            std::sync::Arc::new(false.into());
        let config = TestTrivialConfigProvider {
            local_port,
            target_address: format!("127.0.0.1:{target_port}"),
            should_exit: should_exit.clone(),
        };

        let proxy = tokio::spawn(async move { start(config).await });

        let _ = wait_for_them::wait_for_them(
            &[wait_for_them::ToCheck::HostnameAndPort(
                "127.0.0.1".to_string(),
                local_port,
            )],
            Some(1000),
            None,
            true,
        )
        .await
        .iter()
        .map(|o| o.expect("proxy unreachable"))
        .collect::<Vec<_>>();

        // echo through the proxy
        let (stream_in, mut stream_out) =
            tokio::net::TcpStream::connect(format!("127.0.0.1:{local_port}"))
                .await
                .unwrap()
                .into_split();
        let mut stream_in = tokio::io::BufReader::new(stream_in);
        stream_out.write_all(b"boop\n").await.unwrap();
        let mut line = String::new();
        stream_in.read_line(&mut line).await.unwrap();
        assert!(&line == "boop\n");

        // tell the spawner to exit
        should_exit.store(true, std::sync::atomic::Ordering::Relaxed);

        // wait for the spawner to stop accepting *new* connections
        loop {
            if tokio::net::TcpStream::connect(format!("127.0.0.1:{local_port}"))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // verify that we can still echo through the already-opened connection
        stream_out.write_all(b"boopboop\n").await.unwrap();
        let mut line = String::new();
        stream_in.read_line(&mut line).await.unwrap();
        assert!(&line == "boopboop\n");

        // close the connection
        target.abort_handle().abort();
        let _ = target.await;

        // verify the spawner has exited
        tokio::select!(
            _ = proxy => (),
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(5000)) => {
                assert!(false, "proxy didn't exit!");
            },
        );
    }
}
