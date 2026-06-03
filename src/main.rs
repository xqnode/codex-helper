mod actions;
mod cli;
mod codex;
mod commands;
mod config;
mod env_sync;
mod paths;
mod provider;
mod proxy;
mod settings;

#[cfg(windows)]
mod tray;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("codex_helper=warn".parse().unwrap()))
        .with_target(false)
        .init();

    let cli = cli::Cli::parse();
    if let Err(err) = commands::run(cli).await {
        eprintln!("❌ {err:#}");
        std::process::exit(1);
    }
}
