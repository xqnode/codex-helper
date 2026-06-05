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

#[cfg(windows)]
fn sync_windows_user_env(key: &str, value: &str) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // CREATE_NO_WINDOW = 0x08000000
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = Command::new("setx")
        .args([key, value])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| anyhow::anyhow!("无法执行 setx: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("setx {key} 失败: {stderr}");
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_windows_user_env(_key: &str, _value: &str) -> anyhow::Result<()> {
    Ok(())
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
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = Command::new("cmd")
        .args(["/C", &format!("reg query HKCU\\Environment /v {key}")])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| {
            let line = line.trim();
            if line.starts_with(key) {
                line.split_whitespace().last().map(str::to_string)
            } else {
                None
            }
        })
        .filter(|v| !v.trim().is_empty())
}
