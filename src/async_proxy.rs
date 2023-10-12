use std::net::SocketAddr;
use std::time::Duration;

use crate::config::IntMapping;
use crate::tcp_from_src;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

async fn pipe(mut rx: OwnedReadHalf, mut tx: OwnedWriteHalf) {
    let mut buffer = [0; 16384];
    loop {
        match rx.read(&mut buffer).await {
            Ok(n) => {
                if n == 0 {
                    break;
                }
                if let Err(e) = tx.write_all(&buffer[..n]).await {
                    tracing::info!("Error writing buffer to other stream: {:?}", e);
                    break;
                }
            }
            Err(e) => {
                tracing::info!("Error reading from stream: {:?}", e);
                break;
            }
        };
    }
}

async fn proxy_connection(client: TcpStream, mapping: IntMapping) -> anyhow::Result<()> {
    let upstream = match (
        client.peer_addr(),
        mapping.target_address,
        mapping.transparent,
    ) {
        (Ok(SocketAddr::V4(client_v4)), target_v4, true) => {
            if mapping.connection_is_hairpin(client_v4.ip()) {
                Ok(tokio::net::TcpStream::connect(target_v4).await?)
            } else {
                tcp_from_src::tcpstream_connect_from_addr(client_v4, target_v4).await
            }
        }
        _ => Ok(tokio::net::TcpStream::connect(SocketAddr::V4(mapping.target_address)).await?),
    }?;

    let (in_rx, in_tx) = client.into_split();
    let (out_rx, out_tx) = upstream.into_split();

    tokio::select! {
        _ = tokio::spawn(pipe(in_rx, out_tx)) => (),
        _ = tokio::spawn(pipe(out_rx, in_tx)) => (),
    }

    Ok(())
}

pub async fn start_proxy(
    local_port: u16,
    mapping: IntMapping,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    tracing::info!(
        "staring proxy on port {local_port} to {}:{}. Hairpin net: {:?}",
        mapping.target_address.ip().to_string(),
        mapping.target_address.port(),
        mapping.hairpin_net,
    );
    loop {
        let mut connections = tokio::task::JoinSet::new();
        match TcpListener::bind(format!("0.0.0.0:{local_port}")).await {
            Ok(listener) => loop {
                tokio::select!(
                    _ = cancel.cancelled() => {
                        // after being cancelled, wait until the current set of connections
                        // finished up before returning (note that on this branch, we no longer
                        // accept() new connections!
                        while !connections.is_empty() {
                            connections.join_next().await;
                        }
                        return Ok(());
                    },
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _remote_addr)) => {
                                connections.spawn(proxy_connection(stream, mapping));
                            }
                            Err(e) => {
                                tracing::info!("Error proxying connection: {:?}", e);
                            }
                        }
                    },
                    Some(_) = connections.join_next() => (),
                );
            },
            Err(e) => {
                tracing::info!("failed to bind: {:?}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
