#[macro_use]
extern crate cfg_if;

mod async_proxy;
mod bind_addr;
mod config;
mod iptables_setup;
mod spawner;
mod tcp_from_src;

use crate::config::FileConfigProvider;
use clap::Parser;
use std::path::PathBuf;
use std::process::exit;

/// Standalone runner for Gallium Service Gateway (StarGate)
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to config file
    #[arg(short, long)]
    config: Option<String>,
    /// Print bind addresses and exit
    #[arg(long)]
    bind_addr: bool,
    /// Bind to loopback address
    #[arg(long)]
    bind_loopback: bool,
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();
    let bind_addresses = match bind_addr::get_bind_addresses(args.bind_loopback).await {
        Ok(addrs) => addrs,
        Err(e) => {
            tracing::error!("Error getting bind addresses: {:?}", e);
            exit(1);
        }
    };
    if args.bind_addr {
        println!("{:#?}", bind_addresses);
        return;
    }
    tracing_subscriber::fmt::init();
    let config_path = args.config.unwrap();
    tracing::info!(
        "Starting Gallium Service Gateway with config file: {}",
        config_path
    );

    let config_path = PathBuf::from(&config_path);
    let config_provider = FileConfigProvider {
        config_path,
        bind_loopback: args.bind_loopback,
    };
    spawner::start(config_provider).await;
}
