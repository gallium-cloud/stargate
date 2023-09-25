#[macro_use]
extern crate cfg_if;

mod async_proxy;
mod config;
mod iptables_setup;
mod spawner;
mod tcp_from_src;

use crate::config::FileConfigProvider;
use clap::Parser;
use std::path::PathBuf;

/// Standalone runner for Gallium Service Gateway (StarGate)
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to config file
    #[arg(short, long)]
    config: String,
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();

    let config_path = PathBuf::from(&args.config);
    let config_provider = FileConfigProvider { config_path };
    spawner::start(config_provider).await;
}
