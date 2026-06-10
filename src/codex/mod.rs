mod catalog;
pub mod computer_use;
mod desktop_state;

use toml::map::Map;

use crate::config::{
    self, AppConfig, CC_SWITCH_CODEX_PROVIDER_ID, DUMMY_ENV_KEY, PROVIDER_ID,
};
use crate::paths;

pub fn backup_codex_config() -> anyhow::Result<Option<std::path::PathBuf>> {
    let source = paths::codex_config_path()?;
    if !source.exists() {
        return Ok(None);
    }
    paths::ensure_helper_dirs()?;
    let backup_dir = paths::helper_backups_dir()?;
    let stamp = chrono_like_timestamp();
    let backup = backup_dir.join(format!("config.toml.{stamp}.bak"));
    std::fs::copy(&source, &backup)?;
    Ok(Some(backup))
}

pub fn inject_proxy_config(app: &AppConfig) -> anyhow::Result<()> {
    backup_codex_config()?;
    let mut app = app.clone();
    if app.proxy.host != config::DEFAULT_HOST {
        app.proxy.host = config::DEFAULT_HOST.to_string();
    }
    if app.proxy.port != config::DEFAULT_PORT {
        app.proxy.port = config::DEFAULT_PORT;
    }
    sync_provider_presets(&mut app);
    app.save()?;
    catalog::write_model_catalog(&app)?;

    let provider = app.active_provider()?;
    sync_codex_auth(provider)?;
    ensure_helper_env_keys(provider)?;
    crate::env_sync::sync_codex_desktop_credentials(provider)?;

    let codex_path = paths::codex_config_path()?;
    if let Some(parent) = codex_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = render_codex_config(&app, provider)?;
    config::write_atomic(&codex_path, &content)?;

    let synced = desktop_state::sync_threads_to_config(provider)?;
    if synced > 0 {
        tracing::info!("已同步 {synced} 条 Codex Desktop 会话的 model_provider");
    }

    Ok(())
}

fn bearer_token_for_codex(provider: &config::ProviderConfig) -> anyhow::Result<String> {
    config::resolve_api_key(&provider.api_key_env)
        .or_else(|_| config::resolve_api_key(DUMMY_ENV_KEY))
        .or_else(|_| Ok("local-proxy-placeholder".to_string()))
}

fn sync_codex_auth(provider: &config::ProviderConfig) -> anyhow::Result<()> {
    let token = bearer_token_for_codex(provider)?;
    let auth_path = paths::codex_auth_path()?;
    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let auth = serde_json::json!({
        "OPENAI_API_KEY": token,
        "auth_mode": "apikey",
    });
    let raw = serde_json::to_string_pretty(&auth)?;
    config::write_atomic(&auth_path, &raw)?;
    Ok(())
}

fn ensure_helper_env_keys(provider: &config::ProviderConfig) -> anyhow::Result<()> {
    config::save_env_value(DUMMY_ENV_KEY, "local-proxy-placeholder")?;
    if let Ok(key) = config::resolve_api_key(&provider.api_key_env) {
        config::save_env_value("OPENAI_API_KEY", &key)?;
        config::save_env_value(&provider.api_key_env, &key)?;
    }
    Ok(())
}

pub fn restore_openai_official() -> anyhow::Result<()> {
    backup_codex_config()?;
    let codex_path = paths::codex_config_path()?;
    let content = r#"model_provider = "openai"
model = "gpt-5.4"
"#;
    config::write_atomic(&codex_path, content)?;
    Ok(())
}

/// 恢复 OpenAI 默认配置，并移除 codex-helper 写入的 catalog / .env / 占位 auth。
pub fn reset_desktop_defaults() -> anyhow::Result<()> {
    restore_openai_official()?;
    clear_helper_codex_artifacts()?;
    Ok(())
}

fn clear_helper_codex_artifacts() -> anyhow::Result<()> {
    let home = paths::codex_home_dir()?;
    let dotenv = home.join(".env");
    if dotenv.exists() {
        let content = std::fs::read_to_string(&dotenv).unwrap_or_default();
        if content.contains("codex-helper") {
            let _ = std::fs::remove_file(&dotenv);
        }
    }

    let catalog = paths::codex_catalog_path()?;
    if catalog.exists() {
        let _ = std::fs::remove_file(&catalog);
    }

    let auth_path = paths::codex_auth_path()?;
    if auth_path.exists() {
        let raw = std::fs::read_to_string(&auth_path).unwrap_or_default();
        if raw.contains("local-proxy-placeholder") {
            let _ = std::fs::remove_file(&auth_path);
        }
    }
    Ok(())
}

pub fn codex_config_exists() -> bool {
    paths::codex_config_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

pub fn codex_config_uses_helper() -> bool {
    if read_codex_custom_base_url()
        .ok()
        .is_some_and(|url| is_local_helper_proxy_url(&url))
    {
        return true;
    }
    let Ok(path) = paths::codex_config_path() else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    content.contains(PROVIDER_ID)
}

/// 从已解析的 config.toml 读取 `[model_providers.custom].base_url`。
pub fn read_codex_custom_base_url() -> anyhow::Result<String> {
    let table = load_existing_table()?;
    let providers = table
        .get("model_providers")
        .and_then(|v| v.as_table())
        .ok_or_else(|| anyhow::anyhow!("config.toml 缺少 [model_providers.custom]"))?;
    let custom = providers
        .get(CC_SWITCH_CODEX_PROVIDER_ID)
        .and_then(|v| v.as_table())
        .ok_or_else(|| anyhow::anyhow!("config.toml 缺少 [model_providers.custom]"))?;
    custom
        .get("base_url")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("config.toml 缺少 base_url"))
}

/// 统计原始文件中 `[model_providers.custom]` 出现次数（>1 表示可能有残留块）。
pub fn count_custom_provider_sections() -> anyhow::Result<usize> {
    let path = paths::codex_config_path()?;
    if !path.exists() {
        return Ok(0);
    }
    let content = std::fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter(|line| line.trim() == "[model_providers.custom]")
        .count())
}

pub fn codex_proxy_port_matches(app: &AppConfig) -> bool {
    read_codex_custom_base_url()
        .ok()
        .is_some_and(|url| normalize_proxy_url(&url) == normalize_proxy_url(&app.proxy_base_url()))
}

fn is_local_helper_proxy_url(url: &str) -> bool {
    url.contains("127.0.0.1") && url.contains("/v1")
}

fn normalize_proxy_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

fn render_codex_config(app: &AppConfig, provider: &config::ProviderConfig) -> anyhow::Result<String> {
    let catalog_path = catalog::catalog_path_string()?;
    let _catalog_path_toml = catalog_path.replace('\\', "\\\\");

    let mut merged = load_existing_table()?;

    // CC Switch / Codex Desktop 约定：第三方接入必须用 model_provider = "custom"，
    // 供应商标识写在 [model_providers.custom].name（如 deepseek），才会在模型选择器左侧显示。
    // 若写成 model_provider = "deepseek"，Desktop 不会渲染该标签。
    merged.insert(
        "model_provider".into(),
        toml::Value::String(CC_SWITCH_CODEX_PROVIDER_ID.to_string()),
    );
    merged.insert(
        "preferred_auth_method".into(),
        toml::Value::String("apikey".into()),
    );
    merged.insert(
        "model".into(),
        toml::Value::String(provider.default_model.clone()),
    );
    merged.insert(
        "model_catalog_json".into(),
        toml::Value::String(catalog_path.clone()),
    );
    merged.insert(
        "model_reasoning_effort".into(),
        toml::Value::String(app.normalized_model_reasoning_effort()),
    );
    // 勿开启 disable_response_storage：Codex Desktop 仍依赖 sessions/*.jsonl，
    // 开启后新建对话可能无法生成 rollout 文件，导致「恢复对话失败 / Error submitting message」。
    merged.remove("disable_response_storage");

    // Computer Use / Browser Use 依赖 node_repl MCP，而 node_repl 需要 js_repl 开启。
    // Codex Desktop 在部分场景（第三方 model_provider、Windows sandbox 失败等）会把
    // js_repl 写回 false；helper 每次同步配置时强制打开，避免 $computer-use 报
    //「Node REPL 工具不可用」。
    ensure_desktop_automation_features(&mut merged);

    // 勿写入 sandbox_mode / windows.sandbox / approval_policy：
    // Codex Desktop 会因此出现「自定义 (config.toml)」权限项。
    merged.remove("sandbox_mode");
    merged.remove("approval_policy");
    if let Some(windows) = merged
        .get_mut("windows")
        .and_then(|value| value.as_table_mut())
    {
        windows.remove("sandbox");
        if windows.is_empty() {
            merged.remove("windows");
        }
    }

    let bearer = bearer_token_for_codex(provider)?;

    let mut provider_table = Map::new();
    provider_table.insert(
        "name".into(),
        toml::Value::String(provider.provider_chip_label()),
    );
    provider_table.insert(
        "base_url".into(),
        toml::Value::String(app.proxy_base_url()),
    );
    // 不要写 env_key：Desktop 会强制读进程环境变量并报 Missing OPENAI_API_KEY
    provider_table.insert(
        "experimental_bearer_token".into(),
        toml::Value::String(bearer.clone()),
    );
    // Newer Codex Desktop rejects `wire_api = "chat"` and requires Responses.
    // The local proxy either translates /responses to upstream chat/completions,
    // or passthroughs to upstream /responses when upstream_wire_api = "responses".
    provider_table.insert("wire_api".into(), toml::Value::String("responses".into()));
    // Desktop 在 requires_openai_auth=false 时会隐藏模型选择器
    provider_table.insert(
        "requires_openai_auth".into(),
        toml::Value::Boolean(true),
    );

    merged.insert(
        "experimental_bearer_token".into(),
        toml::Value::String(bearer),
    );

    let mut providers = merged
        .remove("model_providers")
        .and_then(|value| value.as_table().cloned())
        .unwrap_or_default();
    scrub_stale_helper_provider_keys(&mut providers);
    providers.insert(
        CC_SWITCH_CODEX_PROVIDER_ID.to_string(),
        toml::Value::Table(provider_table),
    );
    merged.insert("model_providers".into(), toml::Value::Table(providers));

    let rendered = toml::to_string_pretty(&toml::Value::Table(merged))?;
    verify_written_proxy_url(&rendered, &app.proxy_base_url())?;
    Ok(rendered)
}

fn verify_written_proxy_url(content: &str, expected: &str) -> anyhow::Result<()> {
    let value: toml::Value = toml::from_str(content)?;
    let base_url = value
        .get("model_providers")
        .and_then(|v| v.get(CC_SWITCH_CODEX_PROVIDER_ID))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("写入后校验失败：缺少 model_providers.custom.base_url"))?;
    if normalize_proxy_url(base_url) != normalize_proxy_url(expected) {
        anyhow::bail!(
            "写入后校验失败：base_url 为 {base_url}，期望 {expected}"
        );
    }
    Ok(())
}

fn ensure_desktop_automation_features(merged: &mut Map<String, toml::Value>) {
    let mut features = merged
        .remove("features")
        .and_then(|value| value.as_table().cloned())
        .unwrap_or_default();
    for (key, enabled) in [("js_repl", true), ("plugins", true), ("apps", true)] {
        features.insert(key.into(), toml::Value::Boolean(enabled));
    }
    merged.insert("features".into(), toml::Value::Table(features));
}

pub fn read_js_repl_enabled() -> anyhow::Result<bool> {
    let table = load_existing_table()?;
    Ok(table
        .get("features")
        .and_then(|value| value.get("js_repl"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false))
}

pub fn read_node_repl_mcp_configured() -> bool {
    load_existing_table()
        .ok()
        .and_then(|table| {
            table
                .get("mcp_servers")
                .and_then(|value| value.get("node_repl"))
                .and_then(|value| value.get("command"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .is_some_and(|command| !command.is_empty())
}

fn load_existing_table() -> anyhow::Result<Map<String, toml::Value>> {
    let path = paths::codex_config_path()?;
    if !path.exists() {
        return Ok(Map::new());
    }
    let raw = std::fs::read_to_string(&path)?;
    let value: toml::Value = toml::from_str(&raw).unwrap_or(toml::Value::Table(Map::new()));
    Ok(value.as_table().cloned().unwrap_or_default())
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// 仅移除 helper 误写的 provider 键，保留 openai 等用户自有配置。
fn scrub_stale_helper_provider_keys(providers: &mut Map<String, toml::Value>) {
    providers.remove(PROVIDER_ID);
    for preset in crate::provider::presets::builtin_presets() {
        providers.remove(&preset.id);
    }
}

fn sync_provider_presets(app: &mut AppConfig) {
    crate::provider::sync_builtin_presets(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_proxy_url_strips_trailing_slash() {
        let with_slash = format!("http://{}:{}/v1/", config::DEFAULT_HOST, config::DEFAULT_PORT);
        let without_slash = format!("http://{}:{}/v1", config::DEFAULT_HOST, config::DEFAULT_PORT);
        assert_eq!(normalize_proxy_url(&with_slash), without_slash);
        assert_eq!(normalize_proxy_url("http://127.0.0.1:25543/v1/"), without_slash);
    }

    #[test]
    fn ensure_js_repl_enabled_overrides_false_and_preserves_other_flags() {
        let mut merged = Map::new();
        let mut features = Map::new();
        features.insert("js_repl".into(), toml::Value::Boolean(false));
        features.insert("plugins".into(), toml::Value::Boolean(true));
        merged.insert("features".into(), toml::Value::Table(features));

        ensure_desktop_automation_features(&mut merged);

        let features = merged
            .get("features")
            .and_then(|value| value.as_table())
            .expect("features table");
        assert_eq!(
            features.get("js_repl").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            features.get("plugins").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            features.get("apps").and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn scrub_preserves_unrelated_providers() {
        let mut providers = Map::new();
        providers.insert("openai".into(), toml::Value::String("keep".into()));
        providers.insert("deepseek".into(), toml::Value::String("remove".into()));
        providers.insert(PROVIDER_ID.into(), toml::Value::String("remove".into()));
        scrub_stale_helper_provider_keys(&mut providers);
        assert!(providers.contains_key("openai"));
        assert!(!providers.contains_key("deepseek"));
        assert!(!providers.contains_key(PROVIDER_ID));
    }
}
