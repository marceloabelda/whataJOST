use super::*;

pub(crate) struct TrayBadgeState {
    pub(crate) base_rgba: Vec<u8>,
    pub(crate) current_count: Mutex<u32>,
}

pub(crate) const DIGIT_PATTERNS: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
];

pub(crate) const PLUS_PATTERN: [u8; 7] = [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000];

#[allow(clippy::too_many_arguments)] // coordenadas + color RGBA del badge
pub(crate) fn set_pixel(rgba: &mut [u8], img_w: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    let i = ((y * img_w + x) * 4) as usize;
    if i + 3 < rgba.len() {
        rgba[i] = r;
        rgba[i + 1] = g;
        rgba[i + 2] = b;
        rgba[i + 3] = a;
    }
}

#[allow(clippy::too_many_arguments)] // coordenadas + color RGBA del badge
pub(crate) fn blend_pixel(rgba: &mut [u8], img_w: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    if a == 0 { return; }
    if a == 255 { set_pixel(rgba, img_w, x, y, r, g, b, 255); return; }
    let i = ((y * img_w + x) * 4) as usize;
    if i + 3 >= rgba.len() { return; }
    let da = rgba[i + 3] as u32;
    let sa = a as u32;
    let oa = sa + da * (255 - sa) / 255;
    if oa == 0 { rgba[i]=0; rgba[i+1]=0; rgba[i+2]=0; rgba[i+3]=0; return; }
    rgba[i]   = ((r as u32 * sa + rgba[i]   as u32 * da * (255 - sa) / 255) / oa) as u8;
    rgba[i+1] = ((g as u32 * sa + rgba[i+1] as u32 * da * (255 - sa) / 255) / oa) as u8;
    rgba[i+2] = ((b as u32 * sa + rgba[i+2] as u32 * da * (255 - sa) / 255) / oa) as u8;
    rgba[i+3] = oa as u8;
}

#[allow(clippy::too_many_arguments)] // coordenadas + color RGBA del badge
pub(crate) fn draw_filled_circle(rgba: &mut [u8], img_w: u32, img_h: u32,
                      cx: f32, cy: f32, radius: f32, r: u8, g: u8, b: u8) {
    let feather = 1.2;
    let min_y = ((cy - radius - feather).max(0.0)) as u32;
    let max_y = ((cy + radius + feather).min(img_h as f32)) as u32;
    let min_x = ((cx - radius - feather).max(0.0)) as u32;
    let max_x = ((cx + radius + feather).min(img_w as f32)) as u32;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if dist >= radius + feather { continue; }
            let alpha = if dist <= radius - 0.5 {
                1.0
            } else {
                ((radius + feather - dist) / (feather + 0.5)).clamp(0.0, 1.0)
            };
            blend_pixel(rgba, img_w, x, y, r, g, b, (alpha * 255.0) as u8);
        }
    }
}

pub(crate) fn draw_char(rgba: &mut [u8], img_w: u32, start_x: f32, start_y: f32,
             pattern: &[u8; 7], scale: u32) {
    let s = scale as f32;
    for row in 0..7u32 {
        let bits = pattern[row as usize];
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                let px = start_x + col as f32 * s;
                let py = start_y + row as f32 * s;
                for dy in 0..scale {
                    for dx in 0..scale {
                        set_pixel(rgba, img_w,
                            (px + dx as f32) as u32,
                            (py + dy as f32) as u32,
                            255, 255, 255, 255);
                    }
                }
            }
        }
    }
}

pub(crate) fn draw_count_text(rgba: &mut [u8], img_w: u32, cx: f32, cy: f32, count: u32) {
    let text: Vec<char> = if count > 9 {
        "9+".chars().collect()
    } else if count == 0 {
        return;
    } else {
        count.to_string().chars().collect()
    };
    let char_w = (5 * TRAY_BADGE_FONT_SCALE) as f32;
    let spacing = TRAY_BADGE_FONT_SCALE as f32;
    let total_w = text.len() as f32 * char_w + (text.len() as f32 - 1.0) * spacing;
    let start_x = cx - total_w / 2.0;
    let text_h = (7 * TRAY_BADGE_FONT_SCALE) as f32;
    let start_y = cy - text_h / 2.0;
    for (i, ch) in text.iter().enumerate() {
        let pat = match ch {
            '0'..='9' => &DIGIT_PATTERNS[*ch as usize - '0' as usize],
            '+' => &PLUS_PATTERN,
            _ => continue,
        };
        draw_char(rgba, img_w, start_x + i as f32 * (char_w + spacing), start_y, pat, TRAY_BADGE_FONT_SCALE);
    }
}

pub(crate) fn regenerate_tray_icon(app: &AppHandle) {
    let badge = app.state::<TrayBadgeState>();
    let count = badge.current_count
        .lock()
        .map(|c| *c)
        .unwrap_or_else(|e| {
            eprintln!("[regenerate_tray_icon] Mutex poisoned: {}", e);
            0
        });
    let mut rgba = badge.base_rgba.clone();

    if count > 0 {
        draw_filled_circle(&mut rgba, 64, 64, TRAY_BADGE_CENTER_X as f32, TRAY_BADGE_CENTER_Y as f32, TRAY_BADGE_RADIUS, TRAY_BADGE_RED, TRAY_BADGE_GREEN, TRAY_BADGE_BLUE);
        draw_count_text(&mut rgba, 64, TRAY_BADGE_CENTER_X as f32, TRAY_BADGE_CENTER_Y as f32, count);
    }

    let tray = app.state::<TrayIcon>();
    let _ = tray.set_icon(Some(Image::new_owned(rgba, 64, 64)));
}

pub(crate) fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let autostart_enabled     = app.autolaunch().is_enabled().unwrap_or(false);
    let notifications_enabled = app.state::<NotificationPopupState>()
        .0
        .lock()
        .map(|s| *s)
        .unwrap_or_else(|e| {
            eprintln!("[build_tray_menu] NotificationPopupState poisoned: {}", e);
            true
        });
    let account2_visible = app.state::<Account2VisibleState>()
        .0
        .lock()
        .map(|s| *s)
        .unwrap_or_else(|e| {
            eprintln!("[build_tray_menu] Account2VisibleState poisoned: {}", e);
            false
        });
    let pdf_external_reader = app.state::<PdfExternalReaderState>()
        .0
        .lock()
        .map(|s| *s)
        .unwrap_or_else(|e| {
            eprintln!("[build_tray_menu] PdfExternalReaderState poisoned: {}", e);
            false
        });

    let show_item      = MenuItem::with_id(app, "show",          t("tray_open"),          true, None::<&str>)?;
    let show2_item     = MenuItem::with_id(app, "show2",         t("tray_open2"), account2_visible, None::<&str>)?;
    let autostart_item = CheckMenuItem::with_id(app, "autostart",t("tray_autostart"),   true, autostart_enabled,     None::<&str>)?;
    let notify_item    = CheckMenuItem::with_id(app, "toggle_notify",t("tray_notify"), true, notifications_enabled, None::<&str>)?;
    let account2_item  = CheckMenuItem::with_id(app, "account2", t("tray_account2"),  true, account2_visible, None::<&str>)?;
    let pdf_reader_item = CheckMenuItem::with_id(app, "toggle_pdf_reader", t("tray_pdf_reader"), true, pdf_external_reader, None::<&str>)?;
    let update_item    = MenuItem::with_id(app, "update",         t("tray_update"),    true, None::<&str>)?;
    let logs_item      = MenuItem::with_id(app, "logs",           t("tray_logs"),                true, None::<&str>)?;
    let about_item     = MenuItem::with_id(app, "about",          t("tray_about"),               true, None::<&str>)?;
    let quit_item      = MenuItem::with_id(app, "quit",           t("tray_quit"),                   true, None::<&str>)?;

    let items: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![
        &show_item, &show2_item, &autostart_item, &notify_item, &account2_item, &pdf_reader_item, &update_item, &logs_item, &about_item, &quit_item,
    ];
    Menu::with_items(app, &items)
}

pub(crate) fn show_about(app: &AppHandle) {
    let version = app.package_info().version.to_string();
    let es = ui_lang_es();
    let handle = app.clone();
    app.dialog()
        .message(if es {
            format!("WhataJOST v{version}\n\nWrapper nativo de WhatsApp Web.\n{GITHUB_URL}")
        } else {
            format!("WhataJOST v{version}\n\nNative WhatsApp Web wrapper.\n{GITHUB_URL}")
        })
        .title(t("tray_about"))
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            if es { "Abrir GitHub" } else { "Open GitHub" }.to_string(),
            if es { "Cerrar" } else { "Close" }.to_string(),
        ))
        .show(move |open| {
            if open {
                open_url_external(&handle, GITHUB_URL);
            }
        });
}

pub(crate) fn update_tray_menu(app: &AppHandle) {
    if let Ok(menu) = build_tray_menu(app) {
        let _ = app.state::<TrayIcon>().set_menu(Some(menu));
    }
}

#[tauri::command]
pub(crate) fn update_unread_count(app: AppHandle, badge_state: State<'_, TrayBadgeState>, count: u32) {
    {
        let mut current = match badge_state.current_count.lock() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[update_unread_count] Mutex poisoned: {}", e);
                return;
            }
        };
        if *current == count { return; }
        *current = count;
    }
    regenerate_tray_icon(&app);
}
