use std::net::SocketAddrV4;

cfg_if! {
    if #[cfg(target_os = "linux")] {
        use anyhow::{anyhow, bail};
        use futures::stream::TryStreamExt;
        use netlink_packet_route::nlas::link::Nla as LinkNla;
        use netlink_packet_route::route::Nla as RouteNla;
        use netlink_packet_route::rule::Nla as RuleNla;
        use netlink_packet_route::{ARPHRD_LOOPBACK, FR_ACT_TO_TBL, RTN_LOCAL, RTPROT_BOOT, RT_SCOPE_HOST};
        use rtnetlink;
        use rtnetlink::{Handle, IpVersion};
    } else {
    }
}

#[cfg(target_os = "linux")]
async fn find_ip_rule(handle: &Handle) -> anyhow::Result<bool> {
    let mut r = handle.rule().get(IpVersion::V4).execute();
    let mut found_rule = false;
    while let Some(msg) = r.try_next().await? {
        if msg.header.table == 100 && msg.header.action == FR_ACT_TO_TBL {
            for nla in &msg.nlas {
                match nla {
                    RuleNla::FwMark(1) => {
                        found_rule = true;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(found_rule)
}

#[cfg(target_os = "linux")]
async fn find_ip_route(handle: &Handle) -> anyhow::Result<bool> {
    let mut r = handle.route().get(IpVersion::V4).execute();
    let mut found_route = false;
    while let Some(msg) = r.try_next().await? {
        if msg.header.table == 100
            && msg.header.scope == RT_SCOPE_HOST
            && msg.header.kind == RTN_LOCAL
        {
            for nla in &msg.nlas {
                match nla {
                    RouteNla::Oif(1) => {
                        found_route = true;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(found_route)
}

#[cfg(target_os = "linux")]
async fn find_loopback_intf(handle: &Handle) -> anyhow::Result<u32> {
    let mut r = handle.link().get().execute();
    let mut intfs = vec![];

    while let Some(msg) = r.try_next().await? {
        if msg.header.link_layer_type == ARPHRD_LOOPBACK {
            let index = msg.header.index;
            let mut name = None;
            for nla in &msg.nlas {
                match nla {
                    LinkNla::IfName(s) => {
                        name = Some(s.clone());
                        break;
                    }
                    _ => {}
                }
            }
            intfs.push((index, name));
        }
    }
    // if there's an interface with idx 1 and name 'lo', use that
    for (idx, name) in &intfs {
        if idx == &1_u32 && name == &Some("lo".to_string()) {
            return Ok(idx.clone());
        }
    }
    // Otherwise, use whichever interface has the name 'lo'
    for (idx, name) in &intfs {
        if name == &Some("lo".to_string()) {
            return Ok(idx.clone());
        }
    }
    // if no index has the name 'lo', grab the first one
    if let Some((idx, Some(_name))) = intfs.get(0) {
        return Ok(idx.clone());
    }
    // no valid interface found
    bail! {"loopback interface not found"};
}

#[cfg(target_os = "linux")]
pub async fn initial_setup() -> anyhow::Result<()> {
    let (connection, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(connection);

    let lo_idx = find_loopback_intf(&handle).await?;

    if find_ip_rule(&handle).await? {
        eprintln!("ip rule already exists, skipping creation");
    } else {
        handle
            .rule()
            .add()
            .table_id(100)
            .action(FR_ACT_TO_TBL)
            .v4()
            .fw_mark(1)
            .execute()
            .await?;
        if find_ip_rule(&handle).await? {
            eprintln!("Rule added successfully");
        } else {
            bail!("Rule not found after successful add");
        }
    }

    if find_ip_route(&handle).await? {
        eprint!("ip route already exists, skipping creation");
    } else {
        handle
            .route()
            .add()
            .v4()
            .table_id(100)
            .output_interface(lo_idx)
            .scope(RT_SCOPE_HOST)
            .protocol(RTPROT_BOOT)
            .kind(RTN_LOCAL)
            .execute()
            .await?;
        if find_ip_route(&handle).await? {
            eprintln!("Route added successfully");
        } else {
            bail!("Route not found after successful add")
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
const TABLE_MANGLE: &str = "mangle";
#[cfg(target_os = "linux")]
const CHAIN_PREROUTING: &str = "PREROUTING";

#[cfg(target_os = "linux")]
fn gen_rule_str(target: &SocketAddrV4) -> String {
    let target_ip = target.ip().to_string();
    let target_port = target.port();
    format!("-p tcp -s {target_ip}/32 --sport {target_port} -j MARK --set-xmark 0x1/0xffffffff")
}

#[cfg(target_os = "linux")]
pub async fn add_iptables_return_rule(target: &SocketAddrV4) -> anyhow::Result<()> {
    let rule = gen_rule_str(target);
    println!("Creating rule '{rule}'");
    let ipt = iptables::new(false).map_err(|e| anyhow!("IPTables Err: {:?}", e))?;
    let add_result = ipt.append_unique(TABLE_MANGLE, CHAIN_PREROUTING, rule.as_str());
    eprintln!("{:?}", add_result);
    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn del_iptables_return_rule(target: &SocketAddrV4) -> anyhow::Result<()> {
    let rule = gen_rule_str(&target);
    let ipt = iptables::new(false).map_err(|e| anyhow!("IPTables Err: {:?}", e))?;
    let del_result = ipt.delete(TABLE_MANGLE, CHAIN_PREROUTING, rule.as_str());
    eprintln!("{:?}", del_result);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub async fn del_iptables_return_rule(_target: &SocketAddrV4) -> anyhow::Result<()> {
    unimplemented!()
}
#[cfg(not(target_os = "linux"))]
pub async fn add_iptables_return_rule(_target: &SocketAddrV4) -> anyhow::Result<()> {
    unimplemented!()
}
#[cfg(not(target_os = "linux"))]
pub async fn initial_setup() -> anyhow::Result<()> {
    unimplemented!()
}
