//! macOS 弹窗（GUI 启动失败时用户看不到终端输出）。

#[cfg(target_os = "macos")]
pub fn info(title: &str, message: &str) {
    show_dialog(title, message);
}

#[cfg(target_os = "macos")]
pub fn error(title: &str, message: &str) {
    show_dialog(title, message);
}

#[cfg(target_os = "macos")]
fn show_dialog(title: &str, message: &str) {
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

#[cfg(not(target_os = "macos"))]
pub fn info(_title: &str, _message: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn error(_title: &str, _message: &str) {}
