//! 应用图标（托盘与设置窗口共用）。

#[cfg(any(windows, target_os = "macos"))]
const TRAY_ICON_SIZE: u32 = 128;
#[cfg(any(windows, target_os = "macos"))]
const WINDOW_ICON_SIZE: u32 = 256;

#[cfg(any(windows, target_os = "macos"))]
mod render {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/icon_render.rs"));
}

#[cfg(any(windows, target_os = "macos"))]
pub fn tray_icon() -> tray_icon::Icon {
    let rgba = render::render_icon_rgba(TRAY_ICON_SIZE);
    tray_icon::Icon::from_rgba(rgba, TRAY_ICON_SIZE, TRAY_ICON_SIZE).expect("tray icon")
}

#[cfg(any(windows, target_os = "macos"))]
pub fn window_icon() -> tao::window::Icon {
    let rgba = render::render_icon_rgba(WINDOW_ICON_SIZE);
    tao::window::Icon::from_rgba(rgba, WINDOW_ICON_SIZE, WINDOW_ICON_SIZE).expect("window icon")
}

/// WebView 创建子窗口后可能覆盖任务栏/窗口图标，需再次设置。
#[cfg(any(windows, target_os = "macos"))]
pub fn apply_window_icon(window: &tao::window::Window) {
    window.set_window_icon(Some(window_icon()));
}
