use std::path::PathBuf;

pub fn helper_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?
        .join(".codex-helper");
    Ok(dir)
}

pub fn helper_config_path() -> anyhow::Result<PathBuf> {
    Ok(helper_dir()?.join("config.json"))
}

pub fn helper_env_path() -> anyhow::Result<PathBuf> {
    Ok(helper_dir()?.join(".env"))
}

pub fn helper_backups_dir() -> anyhow::Result<PathBuf> {
    Ok(helper_dir()?.join("backups"))
}

pub fn cc_switch_catalog_path() -> PathBuf {
    codex_home_dir()
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        })
        .join("cc-switch-model-catalog.json")
}

/// Codex Desktop 只稳定读取 ~/.codex 下的 catalog；勿指向 .codex-helper。
pub fn codex_catalog_path() -> anyhow::Result<PathBuf> {
    Ok(cc_switch_catalog_path())
}

pub fn helper_catalog_path() -> anyhow::Result<PathBuf> {
    Ok(helper_dir()?.join("model-catalog.json"))
}

pub fn helper_logs_dir() -> anyhow::Result<PathBuf> {
    Ok(helper_dir()?.join("logs"))
}

pub fn codex_home_dir() -> anyhow::Result<PathBuf> {
    std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .ok_or_else(|| anyhow::anyhow!("无法定位 Codex 配置目录"))
}

pub fn codex_config_path() -> anyhow::Result<PathBuf> {
    Ok(codex_home_dir()?.join("config.toml"))
}

pub fn codex_auth_path() -> anyhow::Result<PathBuf> {
    Ok(codex_home_dir()?.join("auth.json"))
}

pub fn ensure_helper_dirs() -> anyhow::Result<()> {
    std::fs::create_dir_all(helper_dir()?)?;
    std::fs::create_dir_all(helper_backups_dir()?)?;
    std::fs::create_dir_all(helper_logs_dir()?)?;
    Ok(())
}
