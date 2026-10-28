use super::*;

pub(crate) struct NotificationGenState(pub(crate) AtomicU64);

pub(crate) fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                if let Some(c) = char::from_digit((byte >> 4) as u32, 16) {
                    out.push(c.to_ascii_uppercase());
                }
                if let Some(c) = char::from_digit((byte & 0xf) as u32, 16) {
                    out.push(c.to_ascii_uppercase());
                }
            }
        }
    }
    out
}

/// Notificación nativa del escritorio (D-Bus / libnotify). En Wayland las
/// apps no pueden posicionar sus ventanas, así que el toast HTML salía
/// centrado; la notificación nativa la ubica el escritorio.
///
/// Todas las notificaciones reusan el mismo id (`replaces_id`): se muestra
/// solo la última, igual que el toast. Gracias a eso alcanza con un único
/// thread esperando el click — se queda en `wait_for_action` hasta que el
/// usuario clickea o descarta la notificación, y cubre las que la reemplazan.
/// Slot de notificación nativa por cuenta: id a reemplazar y si ya hay un
/// thread esperando el click. Cada cuenta tiene el suyo para que un mensaje
/// de una no tape la notificación de la otra.
#[cfg(target_os = "linux")]
pub(crate) struct NativeNotifSlot {
    pub(crate) id: Mutex<Option<u32>>,
    pub(crate) waiting: std::sync::atomic::AtomicBool,
}

#[cfg(target_os = "linux")]
impl NativeNotifSlot {
    pub(crate) const fn new() -> Self {
        Self { id: Mutex::new(None), waiting: std::sync::atomic::AtomicBool::new(false) }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn show_native_notification(app: &AppHandle, account2: bool, title: &str, body: &str) {
    static SLOTS: [NativeNotifSlot; 2] = [NativeNotifSlot::new(), NativeNotifSlot::new()];
    let slot = &SLOTS[account2 as usize];

    let mut notification = notify_rust::Notification::new();
    notification
        .appname("WhataJOST")
        .summary(title)
        .body(body)
        .icon("whatajost")
        .hint(notify_rust::Hint::DesktopEntry("whataJOST".to_string()))
        .action("default", "Abrir WhatsApp");
    if let Some(id) = slot.id.lock().ok().and_then(|g| *g) {
        notification.id(id);
    }
    let handle = match notification.show() {
        Ok(h) => h,
        Err(e) => {
            log_message(app, LogLevel::Error, format!("Error al mostrar notificación nativa: {e}"));
            return;
        }
    };
    if let Ok(mut g) = slot.id.lock() {
        *g = Some(handle.id());
    }
    if slot.waiting.swap(true, Ordering::SeqCst) {
        return; // ya hay un thread esperando el click sobre este id
    }
    let app = app.clone();
    std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            if action == "default" {
                if account2 { show_window_2(&app) } else { show_window(&app) }
            }
        });
        // Clickeada o descartada: la próxima notificación usa un id nuevo.
        if let Ok(mut g) = slot.id.lock() {
            *g = None;
        }
        slot.waiting.store(false, Ordering::SeqCst);
    });
}

pub(crate) fn show_notification_internal(app: &AppHandle, account2: bool, title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    {
        show_native_notification(app, account2, title, body);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = account2;
        show_toast_notification(app, title, body);
    }
}

/// Toast HTML propio (ventana `notification`). Se usa fuera de Linux.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn show_toast_notification(app: &AppHandle, title: &str, body: &str) {
    let (x, y) = if let Some(monitor) = target_monitor(app) {
        let scale = monitor.scale_factor();
        let size  = monitor.size();
        let pos   = monitor.position();
        (
            pos.x as f64 / scale + size.width  as f64 / scale - 360.0 - 16.0,
            pos.y as f64 / scale + size.height as f64 / scale - 100.0 - 16.0,
        )
    } else {
        log_message(app, LogLevel::Warn, "Notificación descartada: no se pudo obtener ningún monitor");
        return;
    };
    let path = format!("notification.html?t={}&b={}", url_encode(title), url_encode(body));

    // Cada generación invalida el auto-cierre de la anterior, para poder
    // reusar la ventana (ver más abajo) sin que un timer viejo la oculte.
    let gen_state = app.state::<NotificationGenState>();
    let generation = gen_state.0.fetch_add(1, Ordering::SeqCst) + 1;

    // Reusar la ventana existente si hay una: recargar el contenido de un
    // WebView ya vivo es mucho más barato que destruir y crear una ventana
    // nativa nueva por cada mensaje (relevante en ráfagas de notificaciones).
    if let Some(existing) = app.get_webview_window("notification") {
        let js = format!(
            "window.location.replace({});",
            serde_json::to_string(&path).unwrap_or_else(|e| {
                eprintln!("[show_notification_internal] JSON serialization failed: {}", e);
                "\"/fallback\"".to_string()
            })
        );
        let _ = existing.eval(&js);
        let _ = existing.set_position(tauri::LogicalPosition::new(x, y));
        let _ = existing.show();
    } else if let Err(e) = WebviewWindowBuilder::new(app, "notification", WebviewUrl::App(path.into()))
        .title("").decorations(false).transparent(true).always_on_top(true)
        .skip_taskbar(true).resizable(false).inner_size(360.0, 100.0)
        .position(x, y).focused(false).build()
    {
        log_message(app, LogLevel::Error, format!("Error al crear ventana de notificación: {e}"));
        return;
    }

    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(NOTIFICATION_DISPLAY_TIME_MS));
        let gen_state = app2.state::<NotificationGenState>();
        if gen_state.0.load(Ordering::SeqCst) == generation {
            if let Some(win) = app2.get_webview_window("notification") {
                let _ = win.hide();
            }
        }
    });
}

#[tauri::command]
pub(crate) fn show_notification(app: AppHandle, window: tauri::WebviewWindow, state: State<'_, NotificationPopupState>, title: String, body: String) {
    let is_enabled = state
        .0
        .lock()
        .map(|s| *s)
        .unwrap_or_else(|e| {
            eprintln!("[show_notification] NotificationPopupState poisoned: {}", e);
            true
        });
    if !is_enabled {
        return;
    }
    let account2 = window.label() == "whatsapp-web-2";
    let title = if account2 { format!("{title} (Cuenta 2)") } else { title };
    show_notification_internal(&app, account2, &title, &body);
}

#[tauri::command]
pub(crate) fn close_notification(app: AppHandle) {
    if let Some(win) = app.get_webview_window("notification") {
        let _ = win.hide();
    }
}
