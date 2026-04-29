use std::time::Duration;

use tauri::{
    image::Image,
    menu::{Menu, MenuEvent, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("whatsapp-web") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn create_whatsapp_window(app: &AppHandle) -> tauri::Result<()> {
    let opener = app.clone();
    let window = WebviewWindowBuilder::new(
        app,
        "whatsapp-web",
        WebviewUrl::External("https://web.whatsapp.com/".parse().unwrap()),
    )
    .title("WhataJOST")
    .inner_size(1280.0, 800.0)
    .visible(false)
    .focused(false)
    .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    .on_navigation(move |url| {
        let host = url.host_str().unwrap_or("");
        let is_whatsapp = host.ends_with("whatsapp.com")
            || host.ends_with("whatsapp.net")
            || host.ends_with("facebook.com")
            || host.ends_with("fbcdn.net");
        if is_whatsapp {
            true
        } else {
            let _ = opener.opener().open_url(url.as_str(), None::<&str>);
            false
        }
    })
    .initialization_script(r#"
        (function() {
            const isWhatsApp = (url) =>
                /whatsapp\.(com|net)|facebook\.com|fbcdn\.net/.test(new URL(url).hostname);

            window.open = function(url) {
                if (url) try {
                    if (!isWhatsApp(url)) { window.location.href = url; return null; }
                } catch(_) {}
                return null;
            };

            document.addEventListener('click', function(e) {
                for (const node of e.composedPath()) {
                    if (node.tagName === 'A' && node.href) {
                        try {
                            if (!isWhatsApp(node.href)) {
                                e.preventDefault();
                                e.stopImmediatePropagation();
                                window.location.href = node.href;
                            }
                        } catch(_) {}
                        break;
                    }
                }
            }, true);

            // Fix image paste en WebKitGTK: sobreescribir navigator.clipboard.read()
            // que es la API real que usa WhatsApp Web (los eventos sintéticos tienen
            // isTrusted=false y WhatsApp los ignora).
            // Usa IPC si está disponible, sino usa window.__tauriClipboardImage
            // que Rust inyecta vía eval() como fallback.
            function __makeClipboardItems(b64) {
                var bin = atob(b64);
                var arr = new Uint8Array(bin.length);
                for (var i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
                var blob = new Blob([arr], { type: 'image/png' });
                return [{ types: ['image/png'], getType: function(t) {
                    return t === 'image/png' ? Promise.resolve(blob) : Promise.reject(new Error('not found'));
                }}];
            }

            if (navigator.clipboard) {
                navigator.clipboard.read = async function() {
                    // Vía IPC
                    if (window.__TAURI_INTERNALS__) {
                        try {
                            var b64 = await window.__TAURI_INTERNALS__.invoke('read_clipboard_image');
                            if (b64) return __makeClipboardItems(b64);
                        } catch(e) {}
                    }
                    // Fallback: dato inyectado por Rust vía eval
                    if (window.__tauriClipboardImage) {
                        return __makeClipboardItems(window.__tauriClipboardImage);
                    }
                    return [];
                };
            }

            // Intercept Notification API to show in-app toast
            const _OrigNotification = window.Notification;
            window.Notification = function(title, options) {
                try {
                    window.__TAURI_INTERNALS__.invoke('show_notification', {
                        title: title || '',
                        body: (options && options.body) ? options.body : ''
                    });
                } catch(e) {
                    console.warn('[whataJOST] notification invoke failed:', e);
                }
            };
            window.Notification.permission = 'granted';
            window.Notification.requestPermission = function() {
                return Promise.resolve('granted');
            };
            Object.defineProperty(window.Notification, 'permission', {
                get: () => 'granted',
                configurable: true
            });
        })();
    "#)
    .build()?;

    let win_close = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win_close.hide();
        }
    });

    // Hilo que monitorea el clipboard y empuja la imagen a JS vía eval()
    // para cuando el IPC desde URL externa no está disponible.
    let win_clip = window.clone();
    std::thread::spawn(move || {
        let mut last_len: usize = 0;
        loop {
            std::thread::sleep(Duration::from_millis(800));
            if !win_clip.is_visible().unwrap_or(false) {
                continue;
            }
            match read_clipboard_image() {
                Some(b64) if b64.len() != last_len => {
                    last_len = b64.len();
                    let js = format!("window.__tauriClipboardImage = '{}';", b64);
                    let _ = win_clip.eval(&js);
                }
                None if last_len != 0 => {
                    last_len = 0;
                    let _ = win_clip.eval("window.__tauriClipboardImage = null;");
                }
                _ => {}
            }
        }
    });

    Ok(())
}

fn base64_encode_bytes(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | (*chunk.get(2).unwrap_or(&0) as u32);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[tauri::command]
fn read_clipboard_image() -> Option<String> {
    // Wayland
    if let Ok(out) = std::process::Command::new("wl-paste")
        .args(["--type", "image/png", "--no-newline"])
        .output()
    {
        if out.status.success() && !out.stdout.is_empty() {
            return Some(base64_encode_bytes(&out.stdout));
        }
    }
    // X11
    if let Ok(out) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-o"])
        .output()
    {
        if out.status.success() && !out.stdout.is_empty() {
            return Some(base64_encode_bytes(&out.stdout));
        }
    }
    None
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from_digit((byte >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((byte & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

#[tauri::command]
fn show_notification(app: AppHandle, title: String, body: String) {
    // Close any existing notification first
    if let Some(existing) = app.get_webview_window("notification") {
        let _ = existing.close();
    }

    let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let pos = monitor.position();
        let logical_w = size.width as f64 / scale;
        let logical_h = size.height as f64 / scale;
        let notif_w = 360.0f64;
        let notif_h = 100.0f64;
        let margin = 16.0f64;
        (
            pos.x as f64 / scale + logical_w - notif_w - margin,
            pos.y as f64 / scale + logical_h - notif_h - margin,
        )
    } else {
        return;
    };

    let path = format!(
        "notification.html?t={}&b={}",
        url_encode(&title),
        url_encode(&body)
    );

    let result = WebviewWindowBuilder::new(
        &app,
        "notification",
        WebviewUrl::App(path.into()),
    )
    .title("")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .inner_size(360.0, 100.0)
    .position(x, y)
    .focused(false)
    .build();

    if let Ok(win) = result {
        let win_clone = win.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5500));
            let _ = win_clone.close();
        });
    }
}

#[tauri::command]
fn close_notification(app: AppHandle) {
    if let Some(win) = app.get_webview_window("notification") {
        let _ = win.close();
    }
}

#[tauri::command]
fn open_whatsapp(app: AppHandle) {
    show_window(&app);
    if let Some(win) = app.get_webview_window("notification") {
        let _ = win.close();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .invoke_handler(tauri::generate_handler![
            show_notification,
            close_notification,
            open_whatsapp,
            read_clipboard_image
        ])
        .setup(|app| {
            create_whatsapp_window(app.handle())?;

            let show_item =
                MenuItem::with_id(app, "show", "Abrir WhatsApp", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let icon = Image::from_bytes(include_bytes!("../../public/tray.png"))?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("WhataJOST")
                .show_menu_on_left_click(false)
                .on_menu_event(|app: &AppHandle, event: MenuEvent| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => show_window(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray: &TrayIcon, event: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("whatsapp-web") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        } else {
                            show_window(app);
                        }
                    }
                })
                .build(app)?;

            app.manage(tray);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
