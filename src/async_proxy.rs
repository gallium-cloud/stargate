use std::net::SocketAddr;
use std::time::Duration;

use crate::tcp_from_src;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

async fn pipe(mut rx: OwnedReadHalf, mut tx: OwnedWriteHalf) {
    let mut buffer = [0; 4096];
    loop {
        match rx.read(&mut buffer).await {
            Ok(n) => {
                if n == 0 {
                    break;
                }
                if let Err(e) = tx.write(&buffer[..n]).await {
                    eprintln!("Error writing buffer to other stream: {:?}", e);
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error reading from stream: {:?}", e);
                break;
            }
        };
    }
}

async fn proxy_connection(
    client: TcpStream,
    target: SocketAddr,
    transparent: bool,
) -> anyhow::Result<()> {
    let upstream = match (client.peer_addr(), target, transparent) {
        (Ok(SocketAddr::V4(client_v4)), SocketAddr::V4(target_v4), true) => {
            tcp_from_src::tcpstream_connect_from_addr(client_v4, target_v4).await
        }
        _ => Ok(tokio::net::TcpStream::connect(target).await?),
    }?;

    let (in_rx, in_tx) = client.into_split();
    let (out_rx, out_tx) = upstream.into_split();

    tokio::spawn(pipe(in_rx, out_tx));
    tokio::spawn(pipe(out_rx, in_tx));

    Ok(())
}

pub async fn start_proxy(
    local_port: u16,
    target_addr: SocketAddr,
    transparent: bool,
) -> anyhow::Result<()> {
    eprintln!(
        "staring proxy on port {local_port} to {}:{}",
        target_addr.ip().to_string(),
        target_addr.port()
    );
    loop {
        match TcpListener::bind(format!("0.0.0.0:{local_port}")).await {
            Ok(listener) => loop {
                match listener.accept().await {
                    Ok((stream, _remote_addr)) => {
                        tokio::spawn(proxy_connection(stream, target_addr, transparent));
                    }
                    Err(e) => {
                        eprintln!("Error proxying connection: {:?}", e);
                    }
                };
            },
            Err(e) => {
                eprintln!("failed to bind: {:?}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
