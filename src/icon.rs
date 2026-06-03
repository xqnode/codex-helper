//! 应用图标（托盘与设置窗口共用）。

#[cfg(windows)]
const ICON_SIZE: u32 = 32;

#[cfg(windows)]
pub fn tray_icon() -> tray_icon::Icon {
    let (rgba, size) = app_icon_rgba();
    tray_icon::Icon::from_rgba(rgba, size, size).expect("tray icon")
}

#[cfg(windows)]
pub fn window_icon() -> tao::window::Icon {
    let (rgba, size) = app_icon_rgba();
    tao::window::Icon::from_rgba(rgba, size, size).expect("window icon")
}

#[cfg(windows)]
fn app_icon_rgba() -> (Vec<u8>, u32) {
    let size = ICON_SIZE;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    // 蓝色圆底 #2563EB
    let blue = (37u8, 99, 235);
    // 琥珀金闪电 #E8A317，高光 #F5C84A
    let amber_gold = (232u8, 163, 23);
    let amber_gold_hi = (245u8, 200, 74);

    let (cx, cy) = (15.5f32, 15.5f32);
    let radius = 15.5f32;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= radius * radius {
                icon_put_pixel(&mut rgba, x, y, size, blue, 255);
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

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if point_in_polygon(px, py, &bolt) {
                let color = if py < 13.5 {
                    amber_gold_hi
                } else {
                    amber_gold
                };
                icon_put_pixel(&mut rgba, x, y, size, color, 255);
            }
        }
    }

    (rgba, size)
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
