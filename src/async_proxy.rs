use std::net::SocketAddr;
use std::time::Duration;

use crate::config::IntMapping;
use crate::tcp_helper;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

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
                tcp_helper::tcpstream_connect_from_addr(client_v4, target_v4).await
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
    mapping: IntMapping,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    tracing::info!(
        "staring proxy on addrs {}:{} to {}:{}. Hairpin net: {:?}",
        mapping.local_bind.ip(),
        mapping.local_bind.port(),
        mapping.target_address.ip().to_string(),
        mapping.target_address.port(),
        mapping.hairpin_net,
    );
    loop {
        let mut connections = tokio::task::JoinSet::new();
        match tcp_helper::bind_reuseport(mapping.local_bind).await {
            Ok(listener) => loop {
                tokio::select!(
                    _ = cancel.cancelled() => {
                        drop(listener);
                        // after being cancelled & dropping the listener, wait until the
                        // current set of connections finish up before returning.
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
