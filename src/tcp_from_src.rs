use std::net::SocketAddrV4;
use tokio::net::TcpStream;

cfg_if! {
    if #[cfg(target_os = "linux")] {
        use nix::sys::socket::sockopt::{IpTransparent, ReuseAddr};
        use nix::sys::socket::{AddressFamily, SockFlag, SockProtocol, SockType, SockaddrIn};
        use std::io;
        use std::os::fd::AsRawFd;
    } else {
    }
}

#[cfg(target_os = "linux")]
fn sync_tcp_connect(
    bind_addr: SockaddrIn,
    target_sockaddr: SockaddrIn,
) -> io::Result<std::net::TcpStream> {
    let sock_fd = nix::sys::socket::socket(
        AddressFamily::Inet,
        SockType::Stream,
        SockFlag::empty(),
        Some(SockProtocol::Tcp),
    )?;

    nix::sys::socket::setsockopt(&sock_fd, ReuseAddr, &true)?;
    nix::sys::socket::setsockopt(&sock_fd, IpTransparent, &true)?;
    let raw_fd = sock_fd.as_raw_fd();

    nix::sys::socket::bind(raw_fd.clone(), &bind_addr)?;
    nix::sys::socket::connect(raw_fd.clone(), &target_sockaddr)?;
    let tcpstream = std::net::TcpStream::from(sock_fd);
    tcpstream.set_nonblocking(true)?;
    Ok(tcpstream)
}
#[cfg(target_os = "linux")]
pub async fn tcpstream_connect_from_addr(
    bind_addr: SocketAddrV4,
    target_addr: SocketAddrV4,
) -> anyhow::Result<TcpStream> {
    let bind = SockaddrIn::from(bind_addr);
    let target = SockaddrIn::from(target_addr);
    let outgoing_std: std::net::TcpStream =
        tokio::task::spawn_blocking(move || sync_tcp_connect(bind, target)).await??;
    Ok(TcpStream::from_std(outgoing_std)?)
}

#[cfg(not(target_os = "linux"))]
pub async fn tcpstream_connect_from_addr(
    _bind_addr: SocketAddrV4,
    _target_addr: SocketAddrV4,
) -> anyhow::Result<TcpStream> {
    unimplemented!()
}
