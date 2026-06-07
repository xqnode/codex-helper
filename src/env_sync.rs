use crate::config;
use crate::paths;

/// 让 Codex Desktop 能拿到 API Key：写入 ~/.codex/.env，并在 Windows 写入用户级环境变量。
pub fn sync_codex_desktop_credentials(provider: &config::ProviderConfig) -> anyhow::Result<()> {
    let token = config::resolve_api_key(&provider.api_key_env)
        .or_else(|_| config::resolve_api_key(config::DUMMY_ENV_KEY))?;

    write_codex_dotenv(&token)?;
    sync_windows_user_env("OPENAI_API_KEY", &token)?;
    // 当前进程也带上，便于从托盘启动的子进程继承
    std::env::set_var("OPENAI_API_KEY", &token);
    Ok(())
}

fn write_codex_dotenv(openai_api_key: &str) -> anyhow::Result<()> {
    let codex_home = paths::codex_home_dir()?;
    std::fs::create_dir_all(&codex_home)?;
    let path = codex_home.join(".env");
    let content = format!(
        "# 由 codex-helper 自动生成，供 Codex Desktop / CLI 读取\nOPENAI_API_KEY={openai_api_key}\n"
    );
    config::write_atomic(&path, &content)?;
    Ok(())
}

/// Windows 用户环境变量 REG_SZ 实际上限（UTF-16 代码单元）。
const WINDOWS_USER_ENV_MAX_LEN: usize = 32_767;

#[cfg(windows)]
fn validate_windows_env_entry(key: &str, value: &str) -> anyhow::Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("环境变量名不能为空");
    }
    if key.contains('=') {
        anyhow::bail!("环境变量名不能包含 '='");
    }
    if value.len() > WINDOWS_USER_ENV_MAX_LEN {
        anyhow::bail!(
            "环境变量 {key} 值过长（{} 字符），Windows 上限为 {WINDOWS_USER_ENV_MAX_LEN}",
            value.len()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn sync_windows_user_env(key: &str, value: &str) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    validate_windows_env_entry(key, value)?;

    // CREATE_NO_WINDOW = 0x08000000
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 直接调用 reg.exe（不经 cmd），避免 setx 的 1024 字符上限与特殊字符问题。
    let output = Command::new("reg")
        .args([
            "add",
            r"HKCU\Environment",
            "/v",
            key,
            "/t",
            "REG_SZ",
            "/d",
            value,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| anyhow::anyhow!("无法写入 Windows 用户环境变量: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("写入环境变量 {key} 失败: {stderr}");
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_windows_user_env(_key: &str, _value: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::validate_windows_env_entry;

    #[cfg(windows)]
    #[test]
    fn validate_windows_env_entry_rejects_oversized_values() {
        let huge = "x".repeat(32_768);
        assert!(validate_windows_env_entry("OPENAI_API_KEY", &huge).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn validate_windows_env_entry_accepts_percent_in_value() {
        assert!(validate_windows_env_entry("OPENAI_API_KEY", "sk-%FOO%-bar").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn parse_reg_query_value_reads_reg_sz_payload() {
        let text = "HKEY_CURRENT_USER\\Environment\r\n    OPENAI_API_KEY    REG_SZ    sk-%FOO% token\r\n";
        assert_eq!(
            super::parse_reg_query_value(text, "OPENAI_API_KEY").as_deref(),
            Some("sk-%FOO% token")
        );
    }
}

#[cfg(windows)]
pub fn windows_user_env_is_set(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some()
        || read_windows_user_env(key).is_some()
}

#[cfg(windows)]
fn read_windows_user_env(key: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = Command::new("reg")
        .args(["query", r"HKCU\Environment", "/v", key])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_reg_query_value(&String::from_utf8_lossy(&output.stdout), key)
}

#[cfg(windows)]
fn parse_reg_query_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with(key) {
            continue;
        }
        let rest = line.strip_prefix(key)?.trim();
        let rest = rest.strip_prefix("REG_SZ")?.trim();
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}
