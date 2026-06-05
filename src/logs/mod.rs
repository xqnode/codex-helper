mod api;

#[cfg(any(windows, target_os = "macos"))]
mod window;

pub use api::{logs_bootstrap, logs_clear, logs_page};

#[cfg(any(windows, target_os = "macos"))]
pub use window::{
    close_logs_window, focus_logs_window, open_logs_on_loop, LogsWindow,
};
