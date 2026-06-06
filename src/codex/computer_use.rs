use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use toml::map::Map;

use crate::config;
use crate::paths;

const MARKETPLACE_NAME: &str = "computer-use-local";
const PLUGIN_SELECTOR: &str = "computer-use@computer-use-local";
const STALE_PLUGIN_KEY: &str = "computer-use@openai-bundled";

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
    let codex_cli = find_codex_cli().ok_or_else(|| {
        anyhow::anyhow!(
            "找不到 Codex CLI。请先打开一次 Codex Desktop，或运行 codex-helper init 后再试"
        )
    })?;
    let plugin_source = find_bundled_plugin_source(&codex_cli).ok_or_else(|| {
        anyhow::anyhow!(
            "找不到内置 computer-use 插件。请确认 Codex Desktop 已安装且版本支持 Computer Use"
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
    read_codex_cli_from_config()
        .or_else(discover_codex_cli_from_patched_install)
        .filter(|path| path.is_file())
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

fn discover_codex_cli_from_patched_install() -> Option<PathBuf> {
    let home = paths::codex_home_dir().ok()?;
    let patched_root = home.join("zh-cn-patched");
    if !patched_root.is_dir() {
        return None;
    }
    let mut candidates = Vec::new();
    collect_codex_cli_candidates(&patched_root, &mut candidates);
    candidates.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    candidates.into_iter().find(|path| path.is_file())
}

fn collect_codex_cli_candidates(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_codex_cli_candidates(&path, out);
            continue;
        }
        #[cfg(windows)]
        if path.file_name().and_then(|name| name.to_str()) == Some("codex.exe") {
            out.push(path);
        }
        #[cfg(not(windows))]
        if path.file_name().and_then(|name| name.to_str()) == Some("codex") {
            out.push(path);
        }
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
}
