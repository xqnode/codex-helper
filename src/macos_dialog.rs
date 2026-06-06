//! 跨平台简单弹窗（GUI 用户看不到终端输出时使用）。

#[cfg(target_os = "macos")]
pub fn info(title: &str, message: &str) {
    show_macos_dialog(title, message);
}

#[cfg(target_os = "macos")]
pub fn error(title: &str, message: &str) {
    show_macos_dialog(title, message);
}

#[cfg(windows)]
pub fn info(title: &str, message: &str) {
    show_windows_message_box(title, message);
}

#[cfg(windows)]
pub fn error(title: &str, message: &str) {
    show_windows_message_box(title, message);
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn info(_title: &str, _message: &str) {}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn error(_title: &str, _message: &str) {}

#[cfg(target_os = "macos")]
fn show_macos_dialog(title: &str, message: &str) {
    let title = escape_applescript(title);
    let message = escape_applescript(message);
    let script = format!(
        r#"display dialog "{message}" with title "{title}" buttons {{"OK"}} default button 1"#
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

#[cfg(target_os = "macos")]
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(windows)]
fn show_windows_message_box(title: &str, message: &str) {
    use std::process::Command;
    use std::process::Stdio;

    let title = escape_powershell_single_quoted(title);
    let message = escape_powershell_single_quoted(message);
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         [System.Windows.Forms.MessageBox]::Show('{message}','{title}')"
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(windows)]
fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}
