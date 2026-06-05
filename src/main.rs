#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod cli;
mod codex;
mod commands;
mod config;
mod env_sync;
mod icon;
mod logs;
#[cfg(target_os = "macos")]
mod macos_dialog;
mod paths;
mod provider;
mod proxy;
mod request_log;
mod settings;

#[cfg(any(windows, target_os = "macos"))]
mod tray;

use clap::Parser;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("codex_helper=warn".parse().unwrap()),
        )
        .with_target(false)
        .init();
}

/// macOS 菜单栏托盘必须在 OS 主线程创建；#[tokio::main] 会把任务派到 worker 线程。
#[cfg(target_os = "macos")]
fn main() {
    init_tracing();
    let cli = cli::Cli::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(err) = rt.block_on(commands::run(cli)) {
        let msg = format!("{err:#}");
        eprintln!("❌ {msg}");
        crate::macos_dialog::error("Codex Helper 启动失败", &msg);
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
#[tokio::main]
async fn main() {
    init_tracing();
    let cli = cli::Cli::parse();
    if let Err(err) = commands::run(cli).await {
        eprintln!("❌ {err:#}");
        std::process::exit(1);
    }
}
