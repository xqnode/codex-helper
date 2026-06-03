#[cfg(windows)]
use std::cell::RefCell;
#[cfg(windows)]
use std::rc::Rc;
#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
use tao::event::{Event, StartCause, WindowEvent};
#[cfg(windows)]
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
#[cfg(windows)]
use tao::platform::windows::EventLoopBuilderExtWindows;
#[cfg(windows)]
use tokio::sync::RwLock;
#[cfg(windows)]
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIcon, TrayIconBuilder,
};

#[cfg(windows)]
use crate::actions;
#[cfg(windows)]
use crate::config::{self, AppConfig};
#[cfg(windows)]
use crate::proxy::{self, ProxyState};
#[cfg(windows)]
use crate::settings;

#[cfg(windows)]
enum TrayUserEvent {
    RefreshUi,
    OpenSettings,
}

#[cfg(windows)]
pub async fn run_with_proxy(app: AppConfig) -> anyhow::Result<()> {
    let rt_handle = tokio::runtime::Handle::current();
    let config = Arc::new(RwLock::new(app.clone()));
    let proxy = proxy::spawn_server(app.clone())?;

    let mut builder = EventLoopBuilder::<TrayUserEvent>::with_user_event();
    builder.with_any_thread(true);
    let event_loop = builder.build();
    let loop_proxy = event_loop.create_proxy();

    let menu = build_menu(&app)?;
    let tray = TrayIconBuilder::new()
        .with_icon(app_icon())
        .with_menu(Box::new(menu.clone()))
        .with_tooltip(&tooltip_text(&app))
        .build()?;

    let settings_slot: Rc<RefCell<Option<settings::SettingsWindow>>> =
        Rc::new(RefCell::new(None));

    let ctx = Arc::new(TrayContext {
        config,
        proxy,
        tray,
        loop_proxy,
        rt: rt_handle,
        settings: settings_slot.clone(),
    });

    let menu_channel = MenuEvent::receiver();
    let ctx_for_loop = ctx.clone();

    let model = app
        .active_provider()
        .map(|p| p.name.as_str())
        .unwrap_or("?");
    println!("✅ Codex Helper · {} · {}", model, app.proxy_base_url());

    if settings::needs_first_run_setup() {
        let _ = ctx.loop_proxy.send_event(TrayUserEvent::OpenSettings);
    }

    // 托盘事件循环会阻塞当前线程；block_in_place 让 Tokio 继续在其它 worker 上跑代理
    tokio::task::block_in_place(|| {
        event_loop.run(move |event, elwt, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::NewEvents(StartCause::Init) => {}
                Event::UserEvent(TrayUserEvent::RefreshUi) => {
                    refresh_tray_ui(&ctx_for_loop);
                }
                Event::UserEvent(TrayUserEvent::OpenSettings) => {
                    let mut slot = ctx_for_loop.settings.borrow_mut();
                    if slot.is_some() {
                        settings::focus_settings_window(&slot);
                        return;
                    }
                    let port = ctx_for_loop.rt.block_on(async {
                        ctx_for_loop.config.read().await.proxy.port
                    });
                    match settings::open_settings_on_loop(elwt, port) {
                        Ok(window) => *slot = Some(window),
                        Err(err) if err.to_string().contains("already open") => {
                            settings::focus_settings_window(&slot);
                        }
                        Err(err) => tracing::error!("打开设置窗口: {err:#}"),
                    }
                }
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    let mut slot = ctx_for_loop.settings.borrow_mut();
                    settings::close_settings_window(&mut slot, window_id);
                }
                _ => {}
            }

            if let Ok(menu_event) = menu_channel.try_recv() {
                handle_menu_click(&ctx_for_loop, menu_event.id.0.as_str());
            }
        });
    });

    Ok(())
}

#[cfg(windows)]
struct TrayContext {
    config: Arc<RwLock<AppConfig>>,
    proxy: Arc<ProxyState>,
    tray: TrayIcon,
    loop_proxy: EventLoopProxy<TrayUserEvent>,
    rt: tokio::runtime::Handle,
    settings: Rc<RefCell<Option<settings::SettingsWindow>>>,
}

#[cfg(windows)]
struct TrayWorker {
    config: Arc<RwLock<AppConfig>>,
    proxy: Arc<ProxyState>,
}

#[cfg(windows)]
fn handle_menu_click(ctx: &Arc<TrayContext>, id: &str) {
    if id == "quit" {
        std::process::exit(0);
    }
    if id == "settings" {
        let _ = ctx.loop_proxy.send_event(TrayUserEvent::OpenSettings);
        return;
    }
    if id == "open_helper" {
        let _ = actions::open_helper_dir();
        return;
    }
    if id == "open_codex" {
        let _ = actions::open_codex_dir();
        return;
    }
    if id == "resync" {
        spawn_tray_task(ctx, async move |w| {
            if let Err(err) = actions::resync_codex(&w.config, &w.proxy).await {
                tracing::error!("同步失败: {err:#}");
            }
        });
        return;
    }
    if id == "restore_openai" {
        spawn_tray_task(ctx, async move |_w| {
            if let Err(err) = actions::restore_openai().await {
                tracing::error!("恢复失败: {err:#}");
            }
        });
        return;
    }
    if id == "kill_reset_codex" {
        spawn_tray_task(ctx, async move |_w| {
            if let Err(err) = actions::kill_codex_and_reset_defaults().await {
                tracing::error!("退出并恢复默认失败: {err:#}");
            }
        });
        return;
    }
    if let Some(provider_id) = id.strip_prefix("use:") {
        let provider_id = provider_id.to_string();
        spawn_tray_task(ctx, async move |w| {
            match actions::switch_provider(&w.config, &w.proxy, &provider_id).await {
                Ok(()) => tracing::info!("托盘切换完成: {provider_id}"),
                Err(err) => tracing::error!("切换失败: {err:#}"),
            }
        });
    }
}

#[cfg(windows)]
fn spawn_tray_task<F, Fut>(ctx: &Arc<TrayContext>, task: F)
where
    F: FnOnce(TrayWorker) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let worker = TrayWorker {
        config: ctx.config.clone(),
        proxy: ctx.proxy.clone(),
    };
    let loop_proxy = ctx.loop_proxy.clone();
    let rt = ctx.rt.clone();
    rt.spawn(async move {
        task(worker).await;
        let _ = loop_proxy.send_event(TrayUserEvent::RefreshUi);
    });
}

#[cfg(windows)]
fn refresh_tray_ui(ctx: &Arc<TrayContext>) {
    let app = ctx.rt.block_on(async { ctx.config.read().await.clone() });
    if let Ok(menu) = build_menu(&app) {
        let _ = ctx.tray.set_menu(Some(Box::new(menu)));
        let tip = tooltip_text(&app);
        let _ = ctx.tray.set_tooltip(Some(tip.as_str()));
    }
}

#[cfg(windows)]
fn tooltip_text(app: &AppConfig) -> String {
    let name = app
        .active_provider()
        .map(|p| p.name.as_str())
        .unwrap_or("未知");
    if active_key_configured(app) {
        format!("Codex Helper — {name}（运行中）")
    } else {
        format!("Codex Helper — {name}（需设置 API Key）")
    }
}

#[cfg(windows)]
fn active_key_configured(app: &AppConfig) -> bool {
    app.active_provider()
        .ok()
        .and_then(|p| config::resolve_api_key(&p.api_key_env).ok())
        .is_some()
}

#[cfg(windows)]
fn menu_status_line(app: &AppConfig) -> String {
    let provider = app.active_provider().ok();
    let name = provider.map(|p| p.name.as_str()).unwrap_or("未知");
    let model = provider
        .and_then(|p| crate::provider::models::find_model(&p.id, &p.default_model))
        .map(|m| m.display_name)
        .unwrap_or("");
    if active_key_configured(app) {
        if model.is_empty() {
            format!("✓ {name} · 运行中")
        } else {
            format!("✓ {name} · {model}")
        }
    } else if model.is_empty() {
        format!("⚠ {name} · 请先设置 API Key")
    } else {
        format!("⚠ {name} · {model}")
    }
}

#[cfg(windows)]
fn build_menu(app: &AppConfig) -> anyhow::Result<Menu> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id("header", "Codex Helper", false, None))?;
    menu.append(&MenuItem::with_id(
        "status",
        menu_status_line(app),
        false,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;

    let active_name = app
        .active_provider()
        .map(|p| p.name.as_str())
        .unwrap_or("未选择");
    let model_menu = Submenu::with_id(
        "models",
        format!("切换模型  ·  {active_name}"),
        true,
    );
    for preset in crate::provider::list_presets(app) {
        let label = if preset.id == app.active {
            format!("✓ {}", preset.name)
        } else {
            preset.name.clone()
        };
        model_menu.append(&MenuItem::with_id(
            format!("use:{}", preset.id),
            label,
            true,
            None,
        ))?;
    }
    menu.append(&model_menu)?;

    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id("hdr_setup", "常用", false, None))?;
    menu.append(&MenuItem::with_id(
        "settings",
        "设置…",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        "resync",
        "重新同步配置",
        true,
        None,
    ))?;

    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id("hdr_more", "更多", false, None))?;
    menu.append(&MenuItem::with_id(
        "open_helper",
        "打开配置文件夹",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        "open_codex",
        "打开 Codex 文件夹",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        "restore_openai",
        "切换回 OpenAI 官方",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        "kill_reset_codex",
        "重置 Codex 为默认设置",
        true,
        None,
    ))?;

    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id("quit", "退出 Codex Helper", true, None))?;
    Ok(menu)
}

#[cfg(windows)]
fn app_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    // 蓝色圆底 #2563EB
    let blue = (37u8, 99, 235);
    // 琥珀金闪电 #E8A317，高光 #F5C84A
    let amber_gold = (232u8, 163, 23);
    let amber_gold_hi = (245u8, 200, 74);

    let (cx, cy) = (15.5f32, 15.5f32);
    // 蓝底与闪电整体放大，更贴近视图区域
    let radius = 15.5f32;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= radius * radius {
                icon_put_pixel(&mut rgba, x, y, SIZE, blue, 255);
            }
        }
    }

    let bolt = scale_polygon(
        &[
            (16.0, 5.5),
            (10.5, 16.5),
            (14.0, 16.5),
            (9.5, 26.5),
            (22.5, 13.0),
            (17.5, 13.0),
            (21.5, 5.5),
        ],
        16.0,
        16.0,
        1.2,
    );

    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if point_in_polygon(px, py, &bolt) {
                let color = if py < 13.5 { amber_gold_hi } else { amber_gold };
                icon_put_pixel(&mut rgba, x, y, SIZE, color, 255);
            }
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).expect("tray icon")
}

#[cfg(windows)]
fn icon_put_pixel(rgba: &mut [u8], x: u32, y: u32, size: u32, rgb: (u8, u8, u8), a: u8) {
    if x >= size || y >= size {
        return;
    }
    let i = ((y * size + x) * 4) as usize;
    rgba[i] = rgb.0;
    rgba[i + 1] = rgb.1;
    rgba[i + 2] = rgb.2;
    rgba[i + 3] = a;
}

#[cfg(windows)]
fn scale_polygon(verts: &[(f32, f32)], cx: f32, cy: f32, scale: f32) -> Vec<(f32, f32)> {
    verts
        .iter()
        .map(|&(x, y)| (cx + (x - cx) * scale, cy + (y - cy) * scale))
        .collect()
}

#[cfg(windows)]
fn point_in_polygon(x: f32, y: f32, verts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = verts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = verts[i];
        let (xj, yj) = verts[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(not(windows))]
pub async fn run_with_proxy(_app: AppConfig) -> anyhow::Result<()> {
    anyhow::bail!("系统托盘目前仅支持 Windows")
}
