use interfaces::{InterfaceFlags, Kind};
use std::net::{IpAddr, Ipv4Addr};

pub async fn get_bind_addresses(bind_loopback: bool) -> anyhow::Result<Vec<Ipv4Addr>> {
    let mut result = vec![];
    let ipnet_lb = ipnet::Ipv4Net::new(Ipv4Addr::new(127, 0, 0, 0), 8).unwrap();
    let intfs = interfaces::Interface::get_all()?;
    for intf in &intfs {
        if intf.flags.contains(InterfaceFlags::IFF_UP) {
            for addr in &intf.addresses {
                if let Kind::Ipv4 = addr.kind {
                    if let Some(IpAddr::V4(v4_addr)) = addr.addr.map(|a| a.ip()) {
                        if bind_loopback || !ipnet_lb.contains(&v4_addr) {
                            result.push(v4_addr);
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}
