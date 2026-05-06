use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, DragDropEvent, Manager, State, WebviewEvent, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use base64::Engine;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("whatsapp-web") {
        let _ = window.unminimize();
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.set_always_on_top(false);
    }
}

fn check_for_updates(app: &AppHandle) {
    check_for_updates_impl(app, false);
}

fn check_for_updates_impl(app: &AppHandle, silent: bool) {
    let handle = app.clone();
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async {
            let updater = match handle.updater() {
                Ok(u) => u,
                Err(e) => {
                    if !silent {
                        handle
                            .dialog()
                            .message(format!("Error al iniciar el updater: {e}"))
                            .title("whataJOST")
                            .kind(MessageDialogKind::Error)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    return;
                }
            };

            let update = match updater.check().await {
                Ok(Some(u)) => u,
                Ok(None) => {
                    if !silent {
                        handle
                            .dialog()
                            .message("Ya tenés la última versión.")
                            .title("whataJOST")
                            .kind(MessageDialogKind::Info)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    return;
                }
                Err(e) => {
                    if !silent {
                        handle
                            .dialog()
                            .message(format!("Error al buscar actualizaciones: {e}"))
                            .title("whataJOST")
                            .kind(MessageDialogKind::Error)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    return;
                }
            };

            let version = update.version.clone();

            handle
                .dialog()
                .message(format!(
                    "Nueva versión disponible: {version}\n\n¿Querés actualizar ahora?"
                ))
                .title("whataJOST - Actualización")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancel)
                .show(move |result| {
                    if result {
                        let h = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            // Descargar el paquete
                            let bytes = match update.download(|_, _| {}, || {}).await {
                                Ok(b) => b,
                                Err(e) => {
                                    h.dialog()
                                        .message(format!("Error al descargar: {e}"))
                                        .title("whataJOST - Error")
                                        .kind(MessageDialogKind::Error)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                    return;
                                }
                            };

                            // Guardar en un archivo temporal
                            #[cfg(target_os = "linux")]
                            let tmp = std::env::temp_dir().join(format!(
                                "whatajost_update_{}.deb",
                                update.version
                            ));
                            #[cfg(target_os = "windows")]
                            let tmp = std::env::temp_dir().join(format!(
                                "whatajost_update_{}.msi",
                                update.version
                            ));
                            if let Err(e) = std::fs::write(&tmp, &bytes) {
                                h.dialog()
                                    .message(format!("Error al guardar el paquete: {e}"))
                                    .title("whataJOST - Error")
                                    .kind(MessageDialogKind::Error)
                                    .buttons(MessageDialogButtons::Ok)
                                    .show(|_| {});
                                return;
                            }

                            #[cfg(target_os = "linux")]
                            let install_result = std::process::Command::new("pkexec")
                                .arg("dpkg")
                                .arg("-i")
                                .arg(&tmp)
                                .status();

                            #[cfg(target_os = "windows")]
                            let install_result = std::process::Command::new("msiexec")
                                .arg("/i")
                                .arg(&tmp)
                                .arg("/quiet")
                                .status();

                            match install_result {
                                Ok(status) if status.success() => {
                                    // Relanzar: en Linux usamos un shell con delay para
                                    // que single-instance del proceso viejo libere el lock
                                    #[cfg(target_os = "linux")]
                                    if let Ok(exe) = std::env::current_exe() {
                                        let _ = std::process::Command::new("sh")
                                            .arg("-c")
                                            .arg(format!("sleep 2 && exec '{}'", exe.display()))
                                            .stdin(std::process::Stdio::null())
                                            .stdout(std::process::Stdio::null())
                                            .stderr(std::process::Stdio::null())
                                            .spawn();
                                    }
                                    #[cfg(target_os = "windows")]
                                    if let Ok(exe) = std::env::current_exe() {
                                        let mut cmd = std::process::Command::new(&exe);
                                        cmd.stdin(std::process::Stdio::null())
                                            .stdout(std::process::Stdio::null())
                                            .stderr(std::process::Stdio::null());
                                        let _ = cmd.spawn();
                                    }
                                    h.exit(0);
                                }
                                Ok(status) => {
                                    h.dialog()
                                        .message(format!(
                                            "La instalación terminó con código: {}",
                                            status.code().unwrap_or(-1)
                                        ))
                                        .title("whataJOST - Error")
                                        .kind(MessageDialogKind::Error)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                }
                                Err(e) => {
                                    h.dialog()
                                        .message(format!("Error al instalar: {e}"))
                                        .title("whataJOST - Error")
                                        .kind(MessageDialogKind::Error)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                }
                            };
                        });
                    }
                });
        });
    });
}

fn create_whatsapp_window(app: &AppHandle) -> tauri::Result<()> {
    let opener = app.clone();
    let window = WebviewWindowBuilder::new(
        app,
        "whatsapp-web",
        WebviewUrl::External("https://web.whatsapp.com/".parse().unwrap()),
    )
    .title(format!("WhataJOST v{}", env!("CARGO_PKG_VERSION")))
    .inner_size(1280.0, 800.0)
    .visible(false)
    .focused(false)
    .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    .on_navigation(move |url| {
        // Allow blob: and data: URLs (used for file downloads, media, PDFs, etc.)
        let scheme = url.scheme();
        if scheme == "blob" || scheme == "data" || scheme == "about" {
            return true;
        }
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

            // Helper: fetch blob as base64 and invoke save_file
            const downloadBlob = (url, fileName) => {
                (async () => {
                    try {
                        const response = await fetch(url);
                        if (!response.ok) throw new Error('fetch failed');
                        const blob = await response.blob();
                        const base64 = await new Promise((resolve, reject) => {
                            const reader = new FileReader();
                            reader.onload = () => resolve(reader.result.split(',')[1]);
                            reader.onerror = reject;
                            reader.readAsDataURL(blob);
                        });
                        await window.__TAURI_INTERNALS__.invoke('save_file', {
                            data: base64,
                            fileName: fileName || 'archivo'
                        });
                    } catch(e) {
                        console.warn('[whataJOST] download failed:', e);
                    }
                })();
            };

            document.addEventListener('click', function(e) {
                for (const node of e.composedPath()) {
                    if (node.tagName === 'A' && node.href) {
                        try {
                            if (node.download && node.href.startsWith('blob:')) {
                                e.preventDefault();
                                e.stopImmediatePropagation();
                                downloadBlob(node.href, node.download);
                                break;
                            }
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

            // Interceptar evento paste para pegar imágenes desde el portapapeles del sistema
            document.addEventListener('paste', async function(e) {
                // Si el evento ya trae archivos, no interferir
                if (e.clipboardData && e.clipboardData.files && e.clipboardData.files.length > 0) {
                    return;
                }

                // Obtener imagen del portapapeles via IPC o variable inyectada por Rust
                let b64 = null;
                if (window.__TAURI_INTERNALS__) {
                    try {
                        b64 = await window.__TAURI_INTERNALS__.invoke('read_clipboard_image');
                    } catch(err) {}
                }
                if (!b64 && window.__tauriClipboardImage) {
                    b64 = window.__tauriClipboardImage;
                }
                if (!b64) return; // no hay imagen en el portapapeles

                e.preventDefault();
                e.stopPropagation();

                // Convertir base64 a File
                const binary = atob(b64);
                const array = new Uint8Array(binary.length);
                for (let i = 0; i < binary.length; i++) array[i] = binary.charCodeAt(i);
                const blob = new Blob([array], { type: 'image/png' });
                const file = new File([blob], 'clipboard.png', { type: 'image/png' });

                // Crear DataTransfer con el archivo y despachar un paste sintético.
                // Así WhatsApp Web lo procesa como imagen normal (no como sticker,
                // que es lo que ocurre al inyectar vía <input type="file"> + change).
                const dt = new DataTransfer();
                dt.items.add(file);

                let pasteEvent;
                try {
                    pasteEvent = new ClipboardEvent('paste', {
                        bubbles: true,
                        cancelable: true,
                        clipboardData: dt
                    });
                } catch(_) {
                    pasteEvent = new Event('paste', { bubbles: true, cancelable: true });
                    Object.defineProperty(pasteEvent, 'clipboardData', {
                        get: function() { return dt; },
                        configurable: true
                    });
                }

                const target = document.activeElement || document.body;
                target.dispatchEvent(pasteEvent);
            }, true);

            // Interceptar drag & drop para pegar archivos en la conversación
            document.addEventListener('dragover', function(e) {
                e.preventDefault();
                e.stopPropagation();
            }, true);

            document.addEventListener('drop', function(e) {
                e.preventDefault();
                e.stopPropagation();

                if (!e.dataTransfer || !e.dataTransfer.files || e.dataTransfer.files.length === 0) {
                    return;
                }

                const dt = new DataTransfer();
                for (let i = 0; i < e.dataTransfer.files.length; i++) {
                    dt.items.add(e.dataTransfer.files[i]);
                }

                let pasteEvent;
                try {
                    pasteEvent = new ClipboardEvent('paste', {
                        bubbles: true,
                        cancelable: true,
                        clipboardData: dt
                    });
                } catch(_) {
                    pasteEvent = new Event('paste', { bubbles: true, cancelable: true });
                    Object.defineProperty(pasteEvent, 'clipboardData', {
                        get: function() { return dt; },
                        configurable: true
                    });
                }

                const target = document.activeElement || document.body;
                target.dispatchEvent(pasteEvent);
            }, true);

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


            // Watch document.title for unread message count
            (function() {
                let lastCount = 0;
                function checkCount() {
                    const title = document.title;
                    let count = 0;
                    const m = title.match(/^\((\d+)\)\s/);
                    if (m) { count = parseInt(m[1], 10) || 0; }
                    if (count !== lastCount) {
                        lastCount = count;
                        try {
                            window.__TAURI_INTERNALS__.invoke('update_unread_count', { count: count });
                        } catch(e) {}
                    }
                }
                if (document.readyState === 'loading') {
                    document.addEventListener('DOMContentLoaded', checkCount, { once: true });
                } else {
                    checkCount();
                }
                const titleEl = document.querySelector('title');
                if (titleEl) {
                    const obs = new MutationObserver(checkCount);
                    obs.observe(titleEl, { childList: true, characterData: true, subtree: true });
                }
                setInterval(checkCount, 2500);
            })();
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

    // Manejar drag & drop de archivos desde el sistema operativo
    let w = window.clone();
    window.on_webview_event(move |event| {
        if let WebviewEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
            let mut files_json = String::from("[");
            for (i, path) in paths.iter().enumerate() {
                if let Ok(data) = std::fs::read(path) {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if i > 0 {
                            files_json.push(',');
                        }
                        let b64 = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &data,
                        );
                        let escaped_name = name.replace('\\', "\\\\").replace('"', "\\\"");
                        files_json.push_str(&format!(
                            r#"{{"name":"{}","data":"{}"}}"#,
                            escaped_name, b64
                        ));
                    }
                }
            }
            files_json.push(']');

            let _ = w.eval(&format!(
                r#"(function(){{var files={};if(!files.length)return;var dt=new DataTransfer();for(var i=0;i<files.length;i++){{var f=files[i];var b=atob(f.data);var a=new Uint8Array(b.length);for(var j=0;j<b.length;j++)a[j]=b.charCodeAt(j);var blob=new Blob([a],{{type:"application/octet-stream"}});var file=new File([blob],f.name,{{type:"application/octet-stream"}});dt.items.add(file);}}var pe;try{{pe=new ClipboardEvent("paste",{{bubbles:true,cancelable:true,clipboardData:dt}});}}catch(_){{pe=new Event("paste",{{bubbles:true,cancelable:true}});Object.defineProperty(pe,"clipboardData",{{get:function(){{return dt;}},configurable:true}});}}(document.activeElement||document.body).dispatchEvent(pe);}})()"#,
                files_json
            ));
        }
    });

    Ok(())
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
static CLIPBOARD_TOOL: OnceLock<Option<&'static str>> = OnceLock::new();

struct TrayBadgeState {
    base_rgba: Vec<u8>,
    current_count: Mutex<u32>,
}

struct NotificationPopupState(Mutex<bool>);

fn config_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_config_dir().expect("failed to get config dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("config.json")
}

fn load_notification_enabled(app: &AppHandle) -> bool {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("notifications_enabled").and_then(|e| e.as_bool()))
        .unwrap_or(true)
}

fn save_notification_enabled(app: &AppHandle, enabled: bool) {
    let path = config_path(app);
    let json = serde_json::json!({"notifications_enabled": enabled});
    std::fs::write(&path, json.to_string()).ok();
}

// --- tray badge rendering ---

const DIGIT_PATTERNS: [[u8; 7]; 10] = [
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

const PLUS_PATTERN: [u8; 7] = [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000];
const FONT_SCALE: u32 = 2;

fn set_pixel(rgba: &mut [u8], img_w: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    let i = ((y * img_w + x) * 4) as usize;
    if i + 3 < rgba.len() {
        rgba[i] = r;
        rgba[i + 1] = g;
        rgba[i + 2] = b;
        rgba[i + 3] = a;
    }
}

fn blend_pixel(rgba: &mut [u8], img_w: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
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

fn draw_filled_circle(rgba: &mut [u8], img_w: u32, img_h: u32,
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

fn draw_char(rgba: &mut [u8], img_w: u32, start_x: f32, start_y: f32,
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

fn draw_count_text(rgba: &mut [u8], img_w: u32, cx: f32, cy: f32, count: u32) {
    let text: Vec<char> = if count > 9 {
        "9+".chars().collect()
    } else if count == 0 {
        return;
    } else {
        count.to_string().chars().collect()
    };
    let char_w = (5 * FONT_SCALE) as f32;
    let spacing = FONT_SCALE as f32;
    let total_w = text.len() as f32 * char_w + (text.len() as f32 - 1.0) * spacing;
    let start_x = cx - total_w / 2.0;
    let text_h = (7 * FONT_SCALE) as f32;
    let start_y = cy - text_h / 2.0;
    for (i, ch) in text.iter().enumerate() {
        let pat = match ch {
            '0'..='9' => &DIGIT_PATTERNS[*ch as usize - '0' as usize],
            '+' => &PLUS_PATTERN,
            _ => continue,
        };
        draw_char(rgba, img_w, start_x + i as f32 * (char_w + spacing), start_y, pat, FONT_SCALE);
    }
}

fn generate_badged_icon(base_rgba: &[u8], count: u32) -> Option<Vec<u8>> {
    if count == 0 { return None; }
    let mut rgba = base_rgba.to_vec();
    draw_filled_circle(&mut rgba, 64, 64, 48.0, 16.0, 14.0, 255, 59, 48);
    draw_count_text(&mut rgba, 64, 48.0, 16.0, count);
    Some(rgba)
}

#[cfg(target_os = "linux")]
fn read_clipboard_image_raw(tool: &str) -> Option<Vec<u8>> {
    let (cmd, args): (&str, &[&str]) = match tool {
        "wl-paste" => ("wl-paste", &["--type", "image/png", "--no-newline"]),
        _ => ("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"]),
    };
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success() && !out.stdout.is_empty())
        .map(|out| out.stdout)
}

#[cfg(target_os = "linux")]
#[tauri::command]
fn read_clipboard_image() -> Option<String> {
    if let Some(Some(tool)) = CLIPBOARD_TOOL.get() {
        if let Some(data) = read_clipboard_image_raw(tool) {
            return Some(base64_encode_bytes(&data));
        }
    }
    for tool in ["wl-paste", "xclip"] {
        if let Some(data) = read_clipboard_image_raw(tool) {
            let _ = CLIPBOARD_TOOL.set(Some(tool));
            return Some(base64_encode_bytes(&data));
        }
    }
    let _ = CLIPBOARD_TOOL.set(None);
    None
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
fn read_clipboard_image() -> Option<String> {
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
fn show_notification(app: AppHandle, state: State<'_, NotificationPopupState>, title: String, body: String) {
    if !*state.0.lock().unwrap() {
        return;
    }

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
fn save_file(data: String, file_name: String) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Error al decodificar: {e}"))?;

    let path = rfd::FileDialog::new()
        .set_file_name(&file_name)
        .save_file();

    match path {
        Some(p) => {
            std::fs::write(&p, &bytes)
                .map_err(|e| format!("Error al guardar: {e}"))?;
            Ok(p.to_string_lossy().to_string())
        }
        None => Err("Cancelado".to_string()),
    }
}

#[tauri::command]
fn update_unread_count(app: AppHandle, badge_state: State<'_, TrayBadgeState>, count: u32) {
    {
        let mut current = badge_state.current_count.lock().unwrap();
        if *current == count { return; }
        *current = count;
    }
    let tray = app.state::<TrayIcon>();
    let icon = if count == 0 {
        Some(Image::new_owned(
            badge_state.base_rgba.clone(),
            64, 64))
    } else {
        generate_badged_icon(&badge_state.base_rgba, count)
            .map(|rgba| Image::new_owned(rgba, 64, 64))
    };
    if let Err(e) = tray.set_icon(icon) {
        eprintln!("[whataJOST] Failed to update tray icon: {e}");
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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .invoke_handler(tauri::generate_handler![
            show_notification,
            close_notification,
            open_whatsapp,
            read_clipboard_image,
            save_file,
            update_unread_count
        ])
        .setup(|app| {
            let notifications_enabled = load_notification_enabled(app.handle());
            app.manage(NotificationPopupState(Mutex::new(notifications_enabled)));

            create_whatsapp_window(app.handle())?;

            let autostart_enabled = app
                .autolaunch()
                .is_enabled()
                .unwrap_or(false);

            let show_item =
                MenuItem::with_id(app, "show", "Abrir WhatsApp", true, None::<&str>)?;
            let autostart_item = CheckMenuItem::with_id(
                app,
                "autostart",
                "Iniciar con el sistema",
                true,
                autostart_enabled,
                None::<&str>,
            )?;
            let notify_item =
                CheckMenuItem::with_id(app, "toggle_notify", "Notificaciones emergentes", true, notifications_enabled, None::<&str>)?;
            let update_item =
                MenuItem::with_id(app, "update", "Buscar actualización", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_item, &autostart_item, &notify_item, &update_item, &quit_item],
            )?;

            let icon = Image::from_bytes(include_bytes!("../../public/tray.png"))?;

            app.manage(TrayBadgeState {
                base_rgba: icon.rgba().to_vec(),
                current_count: Mutex::new(0),
            });

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("WhataJOST")
                .show_menu_on_left_click(false)
                .on_menu_event(|app: &AppHandle, event: MenuEvent| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => show_window(app),
                    "update" => check_for_updates(app),
                    "toggle_notify" => {
                        let state = app.state::<NotificationPopupState>();
                        let mut enabled = state.0.lock().unwrap();
                        *enabled = !*enabled;
                        save_notification_enabled(app, *enabled);
                    }
                    "autostart" => {
                        let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                        if enabled {
                            let _ = app.autolaunch().disable();
                        } else {
                            let _ = app.autolaunch().enable();
                        }
                    }
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

            // Check for updates in background after app starts
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(5));
                check_for_updates_impl(&handle, true);
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
