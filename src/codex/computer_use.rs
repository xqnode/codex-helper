use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use toml::map::Map;

use crate::config;
use crate::paths;

const MARKETPLACE_NAME: &str = "computer-use-local";
const PLUGIN_SELECTOR: &str = "computer-use@computer-use-local";
const STALE_PLUGIN_KEY: &str = "computer-use@openai-bundled";

#[cfg(windows)]
const CLI_BINARY_NAME: &str = "codex.exe";
#[cfg(not(windows))]
const CLI_BINARY_NAME: &str = "codex";

const MARKETPLACE_JSON: &str = r#"{
  "name": "computer-use-local",
  "interface": {
    "displayName": "Computer Use Local"
  },
  "plugins": [
    {
      "name": "computer-use",
      "source": {
        "source": "local",
        "path": "./plugins/computer-use"
      },
      "policy": {
        "installation": "AVAILABLE",
        "authentication": "ON_USE"
      },
      "category": "Productivity"
    }
  ]
}
"#;

pub fn is_installed() -> bool {
    plugin_cache_root().is_some_and(|p| p.exists()) && config_enables_local_plugin()
}

pub fn repair() -> anyhow::Result<()> {
    let codex_cli = find_codex_cli().ok_or_else(codex_cli_missing_error)?;
    let plugin_source = find_bundled_plugin_source(&codex_cli).ok_or_else(|| {
        anyhow::anyhow!(
            "已找到 Codex CLI，但缺少内置 computer-use 插件。\n\n\
             路径: {}\n\n\
             请更新 Codex Desktop 到支持 Computer Use 的版本后重试。",
            codex_cli.display()
        )
    })?;

    crate::codex::backup_codex_config()?;

    let marketplace_root = marketplace_root()?;
    prepare_local_marketplace(&marketplace_root, &plugin_source)?;

    run_codex_plugin_cmd(&codex_cli, &["plugin", "marketplace", "add", &path_to_arg(&marketplace_root)])?;
    run_codex_plugin_cmd(
        &codex_cli,
        &["plugin", "add", PLUGIN_SELECTOR],
    )?;

    finalize_config()?;

    println!("✅ Computer Use 修复完成");
    println!("   插件: {PLUGIN_SELECTOR}");
    println!("   市场: {MARKETPLACE_NAME}");
    println!();
    println!("请完全退出并重启 Codex Desktop，新开一条对话后使用 $computer-use");
    println!("提示: 插件页里 openai-bundled 的「安装」按钮可能仍会失败，可忽略；以对话里 $computer-use 为准。");
    Ok(())
}

fn config_enables_local_plugin() -> bool {
    load_config_table()
        .ok()
        .and_then(|table| {
            table
                .get("plugins")
                .and_then(|v| v.get(PLUGIN_SELECTOR))
                .and_then(|v| v.get("enabled"))
                .and_then(|v| v.as_bool())
        })
        .unwrap_or(false)
}

fn plugin_cache_root() -> Option<PathBuf> {
    paths::codex_home_dir()
        .ok()
        .map(|home| {
            home.join("plugins")
                .join("cache")
                .join(MARKETPLACE_NAME)
                .join("computer-use")
        })
}

fn marketplace_root() -> anyhow::Result<PathBuf> {
    Ok(paths::codex_home_dir()?
        .join(".tmp")
        .join("bundled-marketplaces")
        .join(MARKETPLACE_NAME))
}

fn prepare_local_marketplace(root: &Path, plugin_source: &Path) -> anyhow::Result<()> {
    let agents_dir = root.join(".agents").join("plugins");
    let plugin_dest = root.join("plugins").join("computer-use");
    fs::create_dir_all(&agents_dir)?;
    if plugin_dest.exists() {
        fs::remove_dir_all(&plugin_dest)?;
    }
    copy_dir_recursive(plugin_source, &plugin_dest)?;
    config::write_atomic(&agents_dir.join("marketplace.json"), MARKETPLACE_JSON)?;
    Ok(())
}

fn finalize_config() -> anyhow::Result<()> {
    let mut merged = load_config_table()?;
    ensure_automation_features(&mut merged);
    ensure_plugin_enabled(&mut merged);
    remove_stale_plugin_entry(&mut merged);
    let content = toml::to_string_pretty(&toml::Value::Table(merged))?;
    config::write_atomic(&paths::codex_config_path()?, &content)?;
    Ok(())
}

fn ensure_automation_features(merged: &mut Map<String, toml::Value>) {
    let mut features = merged
        .remove("features")
        .and_then(|value| value.as_table().cloned())
        .unwrap_or_default();
    for (key, enabled) in [("js_repl", true), ("plugins", true), ("apps", true)] {
        features.insert(key.into(), toml::Value::Boolean(enabled));
    }
    merged.insert("features".into(), toml::Value::Table(features));
}

fn ensure_plugin_enabled(merged: &mut Map<String, toml::Value>) {
    let mut plugins = merged
        .remove("plugins")
        .and_then(|value| value.as_table().cloned())
        .unwrap_or_default();
    let mut plugin = Map::new();
    plugin.insert("enabled".into(), toml::Value::Boolean(true));
    plugins.insert(PLUGIN_SELECTOR.into(), toml::Value::Table(plugin));
    merged.insert("plugins".into(), toml::Value::Table(plugins));
}

fn remove_stale_plugin_entry(merged: &mut Map<String, toml::Value>) {
    if let Some(plugins) = merged.get_mut("plugins").and_then(|v| v.as_table_mut()) {
        plugins.remove(STALE_PLUGIN_KEY);
    }
}

fn load_config_table() -> anyhow::Result<Map<String, toml::Value>> {
    let path = paths::codex_config_path()?;
    if !path.exists() {
        return Ok(Map::new());
    }
    let raw = fs::read_to_string(&path)?;
    let value: toml::Value = toml::from_str(&raw).unwrap_or(toml::Value::Table(Map::new()));
    Ok(value.as_table().cloned().unwrap_or_default())
}

pub fn find_codex_cli() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = read_codex_cli_from_config() {
        candidates.push(path);
    }
    discover_codex_home_candidates(&mut candidates);
    discover_path_candidates(&mut candidates);
    #[cfg(windows)]
    discover_windows_store_candidates(&mut candidates);
    #[cfg(not(windows))]
    discover_macos_app_candidates(&mut candidates);
    pick_best_codex_cli(candidates)
}

/// 供 doctor / 托盘提示：是否能在本机定位到 codex.exe。
pub fn codex_cli_available() -> bool {
    find_codex_cli().is_some()
}

fn codex_cli_missing_error() -> anyhow::Error {
    let config_path = paths::codex_config_path().ok();
    let config_exists = config_path.as_ref().is_some_and(|p| p.exists());
    let node_repl = crate::codex::read_node_repl_mcp_configured();

    let mut msg = format!("找不到 Codex CLI（{CLI_BINARY_NAME}）。\n\n");
    if !config_exists {
        msg.push_str("尚未检测到 ~/.codex/config.toml。\n");
        msg.push_str("请先安装 Codex Desktop，并完整打开一次后再点「修复 Computer Use」。\n");
    } else if !node_repl {
        msg.push_str("config.toml 里还没有 [mcp_servers.node_repl]。\n");
        msg.push_str("请先完整打开并退出一次 Codex Desktop（由 Desktop 写入 node_repl 路径），再重试。\n");
    } else {
        msg.push_str("Desktop 已写入 node_repl，但本机未在常见路径找到 codex.exe。\n");
        msg.push_str("若使用微软商店版，请更新 Codex Helper 到最新版后重试。\n");
    }
    msg.push_str("\n说明：仅安装 Codex Helper 不够，必须先安装并运行过 Codex Desktop。");
    anyhow::anyhow!(msg)
}

fn pick_best_codex_cli(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    let mut unique = Vec::new();
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let normalized = normalize_existing_path(&path);
        if unique.iter().any(|existing| existing == &normalized) {
            continue;
        }
        unique.push(normalized);
    }
    unique.into_iter().max_by_key(|path| codex_cli_candidate_score(path))
}

fn codex_cli_candidate_score(path: &Path) -> (u8, usize) {
    let has_plugin = find_bundled_plugin_source(path).is_some();
    let from_config = read_codex_cli_from_config()
        .is_some_and(|configured| normalize_existing_path(&configured) == normalize_existing_path(path));
    let score = u8::from(has_plugin) * 2 + u8::from(from_config);
    (score, path.components().count())
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn discover_codex_home_candidates(out: &mut Vec<PathBuf>) {
    let Ok(home) = paths::codex_home_dir() else {
        return;
    };
    if !home.is_dir() {
        return;
    }
    collect_codex_cli_candidates(&home, out, 0);
}

fn discover_path_candidates(out: &mut Vec<PathBuf>) {
    #[cfg(windows)]
    {
        append_cli_from_command(out, "where", &["codex.exe"]);
    }
    #[cfg(not(windows))]
    {
        append_cli_from_command(out, "which", &["codex"]);
    }
}

fn append_cli_from_command(out: &mut Vec<PathBuf>, program: &str, args: &[&str]) {
    let output = match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => output,
        _ => return,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let path = PathBuf::from(line.trim());
        if path.is_file() {
            out.push(path);
        }
    }
}

#[cfg(windows)]
fn discover_windows_store_candidates(out: &mut Vec<PathBuf>) {
    let windows_apps = PathBuf::from(r"C:\Program Files\WindowsApps");
    if !windows_apps.is_dir() {
        return;
    }
    let entries = match fs::read_dir(&windows_apps) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("OpenAI.Codex") {
            continue;
        }
        let candidate = path.join("app").join("resources").join("codex.exe");
        if candidate.is_file() {
            out.push(candidate);
        }
    }
}

#[cfg(not(windows))]
fn discover_macos_app_candidates(out: &mut Vec<PathBuf>) {
    let candidate =
        PathBuf::from("/Applications/Codex.app/Contents/Resources/codex");
    if candidate.is_file() {
        out.push(candidate);
    }
}

fn read_codex_cli_from_config() -> Option<PathBuf> {
    let table = load_config_table().ok()?;
    let node_repl = table.get("mcp_servers")?.get("node_repl")?.as_table()?;

    if let Some(env) = node_repl.get("env").and_then(|v| v.as_table()) {
        if let Some(path) = env.get("CODEX_CLI_PATH").and_then(|v| v.as_str()) {
            let path = PathBuf::from(strip_quotes(path));
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let command = node_repl.get("command").and_then(|v| v.as_str())?;
    let command_path = PathBuf::from(strip_quotes(command));
    let resources = command_path.parent()?;
    #[cfg(windows)]
    let candidate = resources.join("codex.exe");
    #[cfg(not(windows))]
    let candidate = resources.join("codex");
    candidate.is_file().then_some(candidate)
}

fn should_skip_discovery_dir(name: &str) -> bool {
    matches!(
        name,
        "plugins" | "log" | "sessions" | "archived_sessions" | "tmp" | ".sandbox" | "vendor_imports"
    )
}

fn collect_codex_cli_candidates(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 10 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if should_skip_discovery_dir(name) {
                continue;
            }
            collect_codex_cli_candidates(&path, out, depth + 1);
            continue;
        }
        if is_codex_cli_binary(&path) {
            out.push(path);
        }
    }
}

fn is_codex_cli_binary(path: &Path) -> bool {
    #[cfg(windows)]
    {
        path.file_name().and_then(|name| name.to_str()) == Some("codex.exe")
    }
    #[cfg(not(windows))]
    {
        path.file_name().and_then(|name| name.to_str()) == Some("codex")
    }
}

pub fn find_bundled_plugin_source(codex_cli: &Path) -> Option<PathBuf> {
    let bundled = bundled_plugin_path(codex_cli);
    bundled
        .join(".codex-plugin")
        .join("plugin.json")
        .is_file()
        .then_some(bundled)
}

fn bundled_plugin_path(codex_cli: &Path) -> PathBuf {
    codex_cli
        .parent()
        .map(|resources| {
            resources
                .join("plugins")
                .join("openai-bundled")
                .join("plugins")
                .join("computer-use")
        })
        .unwrap_or_else(|| PathBuf::from("computer-use"))
}

fn run_codex_plugin_cmd(codex_cli: &Path, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new(codex_cli).args(args).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    if output.status.success()
        || combined.contains("Added marketplace")
        || combined.contains("Added plugin")
        || combined.contains("already")
        || combined.contains("Installed plugin root")
    {
        return Ok(());
    }
    anyhow::bail!(
        "执行 `{} {}` 失败: {}",
        codex_cli.display(),
        args.join(" "),
        combined.trim()
    )
}

fn path_to_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn strip_quotes(value: &str) -> &str {
    value.trim().trim_matches('\'').trim_matches('"')
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_plugin_relative_to_codex_cli() {
        let codex = PathBuf::from(r"C:\Users\me\.codex\app\resources\codex.exe");
        let source = bundled_plugin_path(&codex);
        assert!(source.ends_with(
            r"plugins\openai-bundled\plugins\computer-use"
        ) || source.ends_with("plugins/openai-bundled/plugins/computer-use"));
    }

    #[test]
    fn strip_quotes_trims_wrapping_quotes() {
        assert_eq!(strip_quotes("'C:\\codex.exe'"), r"C:\codex.exe");
    }

    #[test]
    fn skip_heavy_discovery_dirs() {
        assert!(should_skip_discovery_dir("plugins"));
        assert!(should_skip_discovery_dir("log"));
        assert!(!should_skip_discovery_dir("zh-cn-patched"));
    }
}
