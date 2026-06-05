use reqwest::Client;

use crate::cli::{Cli, Commands, EnvAction};
use crate::codex;
use crate::config::{self, AppConfig};
#[cfg(windows)]
use crate::env_sync;
use crate::provider;
use crate::proxy;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let command = cli.command.unwrap_or(default_command());
    match command {
        Commands::Init => cmd_init().await,
        Commands::Start { no_tray } => cmd_start(no_tray).await,
        Commands::Status => cmd_status(),
        Commands::List => cmd_list(),
        Commands::Use { provider } => cmd_use(&provider).await,
        Commands::Test => cmd_test().await,
        Commands::Doctor => cmd_doctor().await,
        Commands::Settings => cmd_settings().await,
        Commands::Env { action } => cmd_env(action),
        Commands::RestoreOpenai => cmd_restore_openai(),
    }
}

fn default_command() -> Commands {
    #[cfg(any(windows, target_os = "macos"))]
    {
        Commands::Start { no_tray: false }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Commands::Start { no_tray: true }
    }
}

async fn cmd_init() -> anyhow::Result<()> {
    crate::paths::ensure_helper_dirs()?;
    let app = if crate::paths::helper_config_path()?.exists() {
        AppConfig::load()?
    } else {
        let app = AppConfig::default();
        app.save()?;
        app
    };
    codex::inject_proxy_config(&app)?;

    println!("✅ 初始化完成");
    println!("   配置目录: {}", crate::paths::helper_dir()?.display());
    println!("   当前模型: {} ({})", app.active, app.active_provider()?.name);
    println!("   代理地址: {}", app.proxy_base_url());
    println!();
    println!("下一步:");
    println!("  1. 菜单栏/托盘 → 设置 API Key（或 codex-helper settings）");
    println!("  2. codex-helper start    # Windows / macOS 默认带菜单栏托盘");
    println!("  3. 完全退出并重新打开 Codex Desktop（需加载新的环境变量）");
    Ok(())
}

async fn ensure_proxy_port_available(app: &AppConfig) -> anyhow::Result<()> {
    let addr = format!("{}:{}", app.proxy.host, app.proxy.port);
    let health_url = format!("http://{addr}/health");

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()?;

    if let Ok(resp) = client.get(&health_url).send().await {
        if resp.status().is_success() {
            anyhow::bail!(
                "端口 {addr} 上已有 Codex Helper 在运行。请在任务栏找到图标 → 退出后再启动，或直接使用托盘切换模型。"
            );
        }
    }

    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(_) => anyhow::bail!(
            "端口 {addr} 已被其他程序占用。请在设置中更换端口，或关闭占用该端口的程序。"
        ),
    }
}

async fn cmd_start(no_tray: bool) -> anyhow::Result<()> {
    let app = AppConfig::load()?;
    codex::inject_proxy_config(&app)?;
    let app = AppConfig::load()?;

    #[cfg(any(windows, target_os = "macos"))]
    if !no_tray {
        ensure_proxy_port_available(&app).await?;
        return crate::tray::run_with_proxy(app).await;
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    if !no_tray {
        println!("⚠️  系统托盘目前仅支持 Windows / macOS，将以 CLI 模式启动代理");
    }

    ensure_proxy_port_available(&app).await?;
    println!(
        "🚀 {} · {} · Ctrl+C 停止",
        app.proxy_base_url(),
        app.active_provider()?.name
    );
    proxy::start_server(app).await
}

fn cmd_status() -> anyhow::Result<()> {
    let app = AppConfig::load()?;
    let provider = app.active_provider()?;
    let key_status = match config::resolve_api_key(&provider.api_key_env) {
        Ok(_) => "已配置",
        Err(_) => "未配置",
    };

    println!("Codex Helper 状态");
    println!("──────────────────────────────");
    println!("当前模型:   {} ({})", app.active, provider.name);
    println!("默认模型:   {}", provider.default_model);
    println!("代理地址:   {}", app.proxy_base_url());
    println!("上游地址:   {}", provider.base_url);
    println!("API Key:    {key_status} ({})", provider.api_key_env);
    println!(
        "Codex 配置: {}",
        if codex::codex_config_uses_helper() {
            if codex::codex_proxy_port_matches(&app) {
                "已指向本地代理（端口一致）"
            } else {
                "已指向本地代理（⚠ 端口不一致，请重新同步）"
            }
        } else {
            "未配置（运行 codex-helper init）"
        }
    );
    Ok(())
}

fn cmd_list() -> anyhow::Result<()> {
    let app = AppConfig::load()?;
    println!("可用模型预设:");
    for preset in provider::list_presets(&app) {
        let mark = if preset.id == app.active { "✓" } else { " " };
        println!(
            "  {mark} {:<10} {:<12} {}",
            preset.id, preset.default_model, preset.name
        );
    }
    Ok(())
}

async fn cmd_use(provider_id: &str) -> anyhow::Result<()> {
    let mut app = AppConfig::load()?;
    provider::get_preset(&app, provider_id)?;
    app.active = provider_id.to_string();
    app.save()?;
    codex::inject_proxy_config(&app)?;

    let provider = app.active_provider()?;
    println!("✅ 已切换到 {} ({})", provider.id, provider.name);
    println!("   默认模型: {}", provider.default_model);
    if proxy::notify_running_proxy_reload(&app).await {
        println!("   代理已热更新");
    } else {
        println!("   代理未运行，下次 start 时生效");
    }
    Ok(())
}

async fn cmd_test() -> anyhow::Result<()> {
    let app = AppConfig::load()?;
    let provider = app.active_provider()?;
    let api_key = config::resolve_api_key(&provider.api_key_env)?;

    print!("正在测试 {} ... ", provider.name);
    match crate::settings::test_api_key(provider, &api_key).await {
        Ok(()) => {
            println!("✅ 成功");
            Ok(())
        }
        Err(err) => {
            println!("❌ 失败");
            Err(err)
        }
    }
}

async fn cmd_settings() -> anyhow::Result<()> {
    let app = AppConfig::load()?;
    let health_url = format!(
        "http://{}:{}/health",
        app.proxy.host, app.proxy.port
    );
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;

    match client.get(&health_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            #[cfg(any(windows, target_os = "macos"))]
            {
                crate::settings::open_settings_window(app.proxy.port);
                println!("✅ 已打开设置窗口");
                Ok(())
            }
            #[cfg(not(any(windows, target_os = "macos")))]
            {
                anyhow::bail!("设置窗口目前仅支持 Windows / macOS。请使用: codex-helper env set DEEPSEEK_API_KEY sk-xxx")
            }
        }
        _ => anyhow::bail!(
            "本地代理未运行。请先运行 codex-helper start，再右键托盘 → 🔑 设置 API Key"
        ),
    }
}

async fn cmd_doctor() -> anyhow::Result<()> {
    let mut ok = true;
    println!("Codex Helper 诊断");
    println!("──────────────────────────────");

    if crate::paths::helper_dir()?.exists() {
        println!("✅ 配置目录存在");
    } else {
        println!("⚠️  配置目录不存在，运行 codex-helper init");
        ok = false;
    }

    if codex::codex_config_exists() {
        println!("✅ Codex 配置文件存在");
    } else {
        println!("⚠️  Codex 配置文件不存在，运行 codex-helper init");
        ok = false;
    }

    if let Ok(key) = config::resolve_api_key("DEEPSEEK_API_KEY").or_else(|_| config::resolve_api_key("OPENAI_API_KEY")) {
        let preview = if key.len() > 8 {
            format!("{}...{}", &key[..4], &key[key.len() - 4..])
        } else {
            "***".into()
        };
        println!("✅ API Key 已配置 ({preview})");
    } else {
        println!("❌ API Key 未配置，请右键托盘 → 🔑 设置 API Key");
        ok = false;
    }

    #[cfg(windows)]
    if env_sync::windows_user_env_is_set("OPENAI_API_KEY") {
        println!("✅ Windows 用户环境变量 OPENAI_API_KEY 已设置（Desktop 需要）");
    } else {
        println!("⚠️  Windows 用户环境变量 OPENAI_API_KEY 未设置，请运行 codex-helper init 后重启 Codex");
        ok = false;
    }

    #[cfg(target_os = "macos")]
    if crate::paths::codex_home_dir()?.join(".env").exists() {
        println!("✅ ~/.codex/.env 已生成（Codex Desktop 从此读取 API Key）");
    } else {
        println!("⚠️  ~/.codex/.env 不存在，请在设置中保存 API Key");
        ok = false;
    }

    #[cfg(not(target_os = "macos"))]
    if crate::paths::codex_home_dir()?.join(".env").exists() {
        println!("✅ ~/.codex/.env 已生成");
    }

    if codex::codex_config_uses_helper() {
        println!("✅ Codex 已指向 Codex Helper 代理");
    } else {
        println!("⚠️  Codex 尚未指向本地代理");
        ok = false;
    }

    let app = AppConfig::load()?;

    if codex::codex_config_uses_helper() {
        match codex::read_codex_custom_base_url() {
            Ok(url) if codex::codex_proxy_port_matches(&app) => {
                println!("✅ Codex config.toml 代理地址与 config.json 一致 ({url})");
            }
            Ok(url) => {
                println!(
                    "❌ Codex config.toml 代理地址不一致: {url} ≠ {}",
                    app.proxy_base_url()
                );
                println!(
                    "   期望端口 {}，请托盘「重新同步配置」后完全退出并重启 Codex Desktop",
                    config::DEFAULT_PORT
                );
                ok = false;
            }
            Err(_) => {
                println!("❌ 无法读取 config.toml 中的 [model_providers.custom].base_url");
                println!("   运行 codex-helper init 修复");
                ok = false;
            }
        }

        match codex::count_custom_provider_sections() {
            Ok(0) => {}
            Ok(1) => {}
            Ok(n) => {
                println!(
                    "⚠️  config.toml 中有 {n} 个 [model_providers.custom] 块（可能有残留）"
                );
                println!("   运行 codex-helper init 或托盘「重新同步 Codex 配置」清理");
                ok = false;
            }
            Err(_) => {}
        }
    }

    match codex::read_js_repl_enabled() {
        Ok(true) => println!("✅ Codex config.toml 已启用 js_repl（Computer Use / Browser Use 需要）"),
        Ok(false) => {
            println!("⚠️  Codex config.toml 中 js_repl = false，Computer Use 会报 Node REPL 不可用");
            println!("   托盘 → 重新同步配置，然后完全退出并重启 Codex Desktop");
            ok = false;
        }
        Err(_) => {}
    }

    if codex::read_node_repl_mcp_configured() {
        println!("✅ Codex 已配置 mcp_servers.node_repl");
    } else {
        println!("⚠️  Codex 未配置 mcp_servers.node_repl（Computer Use 依赖 node_repl MCP）");
        println!("   在 Codex 设置里安装 Computer Use 插件后重启 Desktop，或重新同步配置");
        ok = false;
    }

    let provider = app.active_provider()?;
    match config::resolve_api_key(&provider.api_key_env) {
        Ok(_) => println!("✅ 当前模型 Key 已配置 ({})", provider.api_key_env),
        Err(_) => {
            println!(
                "❌ 当前模型 Key 未配置，请右键托盘 → 🔑 设置 API Key"
            );
            ok = false;
        }
    }

    let health_url = format!(
        "http://{}:{}/health",
        app.proxy.host, app.proxy.port
    );
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    match client.get(&health_url).send().await {
        Ok(resp) if resp.status().is_success() => println!("✅ 本地代理正在运行"),
        _ => {
            println!("⚠️  本地代理未运行，运行: codex-helper start");
            println!(
                "   默认代理地址: http://{}:{}/v1 （健康检查 /health）",
                app.proxy.host, config::DEFAULT_PORT
            );
            ok = false;
        }
    }

    if ok {
        println!();
        println!("一切正常，可以运行 codex 了。");
    } else {
        println!();
        println!("发现一些问题，请按上方提示修复。");
    }
    Ok(())
}

fn cmd_env(action: EnvAction) -> anyhow::Result<()> {
    match action {
        EnvAction::Set { key, value } => {
            config::save_env_value(&key, &value)?;
            println!("✅ 已保存 {key}");
            Ok(())
        }
    }
}

fn cmd_restore_openai() -> anyhow::Result<()> {
    codex::restore_openai_official()?;
    println!("✅ 已恢复 OpenAI 官方 Codex 配置");
    println!("   请重启终端后运行 codex，并按官方流程登录");
    Ok(())
}
