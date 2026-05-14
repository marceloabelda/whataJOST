use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
use webkit2gtk::{SettingsExt, WebContextExt, WebViewExt};

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

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    timestamp: String,
    level: LogLevel,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ClipboardImage {
    data: String,
    mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
struct FileDropResult {
    name: String,
    data: String,
    mime: String,
}

struct LogState(Arc<Mutex<Vec<LogEntry>>>);

fn format_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn log_message(app: &AppHandle, level: LogLevel, message: impl Into<String>) {
    let state = app.state::<LogState>();
    let mut logs = state.0.lock().unwrap();
    logs.push(LogEntry {
        timestamp: format_timestamp(),
        level,
        message: message.into(),
    });
}

fn handle_deep_link(app: &AppHandle, url: &str) {
    // whatsapp://send?phone=5491112345678 → https://web.whatsapp.com/send?phone=5491112345678
    let web_url = format!(
        "https://web.whatsapp.com/{}",
        url.trim_start_matches("whatsapp://")
    );
    log_message(app, LogLevel::Info, format!("Deep link: {} → {}", url, web_url));
    show_window(app);
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        if let Some(win) = app2.get_webview_window("whatsapp-web") {
            let js = format!(
                "window.location.href = {};",
                serde_json::to_string(&web_url).unwrap()
            );
            let _ = win.eval(&js);
        }
    });
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("whatsapp-web") {
        let _ = window.unminimize();
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.set_always_on_top(false);
    } else {
        log_message(app, LogLevel::Error, "No se encontró la ventana de WhatsApp");
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
                            let install_result = {
                                // Devuelve true si el resultado es error O si el exit code es distinto de 0
                                let failed = |r: &std::io::Result<std::process::ExitStatus>| {
                                    r.as_ref().map(|s| !s.success()).unwrap_or(true)
                                };

                                // Intento 1: pkexec apt install (más compatible con Ubuntu moderno)
                                let mut result = std::process::Command::new("pkexec")
                                    .args(["apt", "install", "--yes"])
                                    .arg(&tmp)
                                    .status();

                                // Intento 2: pkexec dpkg -i
                                if failed(&result) {
                                    result = std::process::Command::new("pkexec")
                                        .args(["dpkg", "-i"])
                                        .arg(&tmp)
                                        .status();
                                }

                                // Intento 3: zenity para pedir contraseña + sudo -S
                                if failed(&result) {
                                    let password = std::process::Command::new("zenity")
                                        .args(["--password", "--title", "whataJOST - Instalar actualización"])
                                        .output()
                                        .ok()
                                        .and_then(|out| String::from_utf8(out.stdout).ok())
                                        .map(|s| s.trim().to_string());
                                    if let Some(pass) = password {
                                        result = std::process::Command::new("sudo")
                                            .args(["-S", "dpkg", "-i"])
                                            .arg(&tmp)
                                            .stdin(std::process::Stdio::piped())
                                            .spawn()
                                            .and_then(|mut child| {
                                                use std::io::Write;
                                                let _ = child.stdin.as_mut().unwrap().write_all(pass.as_bytes());
                                                child.wait()
                                            });
                                    }
                                }

                                // Intento 4: abrir el .deb con el instalador gráfico del sistema
                                if failed(&result) {
                                    let _ = std::process::Command::new("xdg-open")
                                        .arg(&tmp)
                                        .spawn();
                                    h.dialog()
                                        .message(format!(
                                            "Se abrió el instalador del sistema con el paquete.\nCompletá la instalación manualmente y reiniciá whataJOST.\n\nArchivo: {}",
                                            tmp.display()
                                        ))
                                        .title("whataJOST - Actualización")
                                        .kind(MessageDialogKind::Info)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                    return;
                                }

                                result
                            };

                            #[cfg(target_os = "windows")]
                            let install_result = std::process::Command::new("msiexec")
                                .arg("/i")
                                .arg(&tmp)
                                .arg("/quiet")
                                .status();

                            match install_result {
                                Ok(status) if status.success() => {
                                    log_message(&h, LogLevel::Info, "Instalación exitosa. Relanzando...");
                                    h.restart();
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
        let scheme = url.scheme();
        if scheme == "data" || scheme == "about" {
            return true;
        }
        // Interceptar navegación a blob: — en lugar de navegar, disparar descarga via JS
        if scheme == "blob" {
            let url_str = url.as_str().to_string();
            if let Some(win) = opener.get_webview_window("whatsapp-web") {
                let safe = url_str.replace('\\', "\\\\").replace('\'', "\\'");
                let _ = win.eval(&format!(
                    "try{{console.log('[whataJOST] blob nav interceptado');if(typeof window.__waDownloadBlob==='function'){{window.__waDownloadBlob('{}');}}}}catch(e){{console.error('[whataJOST] blob nav error:',e);}}",
                    safe
                ));
            }
            return false;
        }
        // Handle files dropped onto the WebView. WebKitGTK intercepts DnD at the
        // native level and tries to navigate to a file:// URL instead of firing DOM
        // drop events. Read the file and inject it into the page ourselves.
        if scheme == "file" {
            // url.path() devuelve /C:/... en Windows — to_file_path() lo convierte correctamente.
            let file_path = match url.to_file_path() {
                Ok(p) => p,
                Err(_) => {
                    log_message(&opener, LogLevel::Error, format!("on_navigation file:// ruta inválida: {}", url));
                    return false;
                }
            };
            log_message(&opener, LogLevel::Info, format!("on_navigation file:// (fallback): {}", file_path.display()));
            let data = match std::fs::read(&file_path) {
                Ok(d) => d,
                Err(e) => {
                    log_message(&opener, LogLevel::Error, format!("on_navigation file:// error leyendo: {e}"));
                    return false;
                }
            };
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            let file_name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("archivo");
            let mime = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string();
            let js = format!(
                "window.__tauriInjectDrop&&window.__tauriInjectDrop({},{},{})",
                serde_json::to_string(file_name).unwrap(),
                serde_json::to_string(&b64).unwrap(),
                serde_json::to_string(&mime).unwrap(),
            );
            if let Some(win) = opener.get_webview_window("whatsapp-web") {
                let _ = win.eval(&js);
            }
            return false;
        }
        let host = url.host_str().unwrap_or("");
        let is_whatsapp = host.ends_with("whatsapp.com")
            || host.ends_with("whatsapp.net")
            || host.ends_with("facebook.com")
            || host.ends_with("fbcdn.net");
        if is_whatsapp {
            true
        } else {
            let url_str = url.as_str().to_string();
            log_message(&opener, LogLevel::Info, format!("Link externo: {}", url_str));
            if let Err(e) = opener.opener().open_url(&url_str, None::<&str>) {
                log_message(&opener, LogLevel::Warn, format!("opener.open_url falló ({}), usando xdg-open...", e));
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open")
                    .arg(&url_str)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            false
        }
    })
    .initialization_script(r#"
        (function() {
            const _ipcOk = !!window.__TAURI_INTERNALS__;
            console.log('[whataJOST] init script cargado. IPC disponible:', _ipcOk);
            if (_ipcOk) {
                window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'Script de inicialización cargado (whataJOST)'
                });
            }

            const isWhatsApp = (url) =>
                /whatsapp\.(com|net)|facebook\.com|fbcdn\.net/.test(new URL(url).hostname);

            // Rastrear el último elemento editable enfocado.
            // Al hacer drag & drop, document.activeElement puede ser body si el foco
            // se perdió durante el arrastre; con este rastreo podemos re-enfocar el
            // input del chat antes de despachar el paste sintético.
            document.addEventListener('focus', function(e) {
                var el = e.target;
                if (el && (el.isContentEditable || el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) {
                    window.__waLastFocusedEditable = el;
                }
            }, true);

            window.dispatchPasteWithFiles = function(dt) {
                if (!dt.files.length) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', { level: 'warn', message: 'dispatchPasteWithFiles: DataTransfer vacío, no se despacha' });
                    return;
                }
                var stored = window.__waLastFocusedEditable;
                var target = (stored && document.contains(stored))
                    ? stored
                    : (document.querySelector('div[contenteditable][data-tab]') ||
                       document.querySelector('div[contenteditable="true"]') ||
                       document.activeElement ||
                       document.body);
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'dispatchPasteWithFiles: ' + dt.files.length + ' archivo(s), target=' + (target.tagName || 'desconocido') + ' editable=' + (target.isContentEditable || false)
                });
                if (target !== document.body && target.focus) try { target.focus(); } catch(_) {}
                var pe;
                try { pe = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt }); }
                catch(_) {
                    pe = new Event('paste', { bubbles: true, cancelable: true });
                    Object.defineProperty(pe, 'clipboardData', { get: function() { return dt; }, configurable: true });
                }
                target.dispatchEvent(pe);
            };

            window.open = function(url) {
                if (url) try {
                    if (url.startsWith('blob:')) {
                        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                            level: 'info',
                            message: 'window.open(blob:) interceptado, descargando'
                        });
                        downloadBlob(url, 'archivo');
                        return null;
                    }
                    if (!isWhatsApp(url)) {
                        window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                            level: 'info',
                            message: 'Link externo interceptado (window.open): ' + url
                        });
                        window.location.href = url;
                        return null;
                    }
                } catch(e) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'error',
                        message: 'Error en window.open: ' + (e.message || e)
                    });
                }
                return null;
            };

            // Track blob URLs to avoid fetch race conditions
            const blobStore = new Map();
            const origCreateObjectURL = URL.createObjectURL;
            URL.createObjectURL = function(blob) {
                const url = origCreateObjectURL.call(URL, blob);
                blobStore.set(url, blob);
                return url;
            };
            const origRevokeObjectURL = URL.revokeObjectURL;
            URL.revokeObjectURL = function(url) {
                setTimeout(() => blobStore.delete(url), 60000);
                origRevokeObjectURL.call(URL, url);
            };

            // Helper: convert blob to base64 and invoke save_file
            const saveBlob = async (blob, fileName) => {
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'saveBlob: leyendo blob (' + blob.size + ' bytes) para ' + fileName
                });
                const base64 = await new Promise((resolve, reject) => {
                    const reader = new FileReader();
                    reader.onload = () => resolve(reader.result.split(',')[1]);
                    reader.onerror = reject;
                    reader.readAsDataURL(blob);
                });
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'saveBlob: invocando save_file para ' + fileName
                });
                await window.__TAURI_INTERNALS__.invoke('save_file', {
                    data: base64,
                    fileName: fileName || 'archivo'
                });
            };

            const downloadBlob = (url, fileName) => {
                console.log('[whataJOST] downloadBlob iniciado:', fileName || '(sin nombre)', '| IPC:', !!window.__TAURI_INTERNALS__);
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'downloadBlob iniciado: ' + (fileName || '(sin nombre)')
                });
                (async () => {
                    try {
                        let blob = blobStore.get(url);
                        if (!blob) {
                            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                                level: 'info',
                                message: 'Blob no en caché, haciendo fetch de URL'
                            });
                            const response = await fetch(url);
                            if (!response.ok) throw new Error('fetch failed: ' + response.status);
                            blob = await response.blob();
                        }
                        await saveBlob(blob, fileName);
                    } catch(e) {
                        try {
                            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                                level: 'error',
                                message: 'Error al descargar archivo ' + fileName + ': ' + (e.message || e)
                            });
                        } catch (_) {
                            console.error('[whataJOST] Error al descargar (IPC no disponible):', e);
                        }
                    }
                })();
            };

            // Exponer downloadBlob globalmente para que Rust pueda llamarla via eval
            window.__waDownloadBlob = (url, fileName) => downloadBlob(url, fileName || 'archivo');

            // Catch programmatic anchor clicks (WhatsApp Web creates <a> elements and calls .click())
            const origAnchorClick = HTMLAnchorElement.prototype.click;
            HTMLAnchorElement.prototype.click = function() {
                // Interceptar cualquier blob: URL (con o sin atributo download)
                if (this.href && this.href.startsWith('blob:')) {
                    console.log('[whataJOST] prototype.click blob interceptado:', this.download || '(sin attr)');
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'info',
                        message: 'Descarga interceptada (prototype.click): ' + (this.download || '(sin attr)')
                    });
                    downloadBlob(this.href, this.download || 'archivo');
                    return;
                }
                return origAnchorClick.call(this);
            };

            // Catch user clicks on <a> elements (for anchors already in the DOM)
            document.addEventListener('click', function(e) {
                for (const node of e.composedPath()) {
                    if (node.tagName === 'A' && node.href) {
                        try {
                            // Interceptar cualquier blob: URL (con o sin atributo download)
                            if (node.href.startsWith('blob:')) {
                                e.preventDefault();
                                e.stopImmediatePropagation();
                                console.log('[whataJOST] click event blob interceptado:', node.download || '(sin attr)');
                                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                                    level: 'info',
                                    message: 'Descarga interceptada (click event): ' + (node.download || '(sin attr)')
                                });
                                downloadBlob(node.href, node.download || 'archivo');
                                break;
                            }
                            if (!isWhatsApp(node.href)) {
                                e.preventDefault();
                                e.stopImmediatePropagation();
                                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                                    level: 'info',
                                    message: 'Link externo interceptado (click): ' + node.href
                                });
                                window.location.href = node.href;
                            }
                        } catch(e) {
                            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                                level: 'error',
                                message: 'Error en click handler: ' + (e.message || e)
                            });
                        }
                        break;
                    }
                }
            }, true);

            // Allow native context menu on editable fields for spellcheck suggestions.
            // WhatsApp Web intercepts right-clicks via contextmenu/mousedown/mouseup
            // to show its formatting menu. We stop those events in capture phase so
            // the native WebKitGTK menu (with spellcheck) can appear instead.
            const isEditable = (el) => {
                if (!el) return false;
                return el.isContentEditable
                    || el.tagName === 'INPUT'
                    || el.tagName === 'TEXTAREA';
            };
            const resolveTarget = (e) =>
                e.target && e.target.nodeType === Node.TEXT_NODE
                    ? e.target.parentElement
                    : e.target;

            document.addEventListener('contextmenu', function(e) {
                if (isEditable(resolveTarget(e))) {
                    e.stopImmediatePropagation();
                }
            }, true);
            document.addEventListener('mousedown', function(e) {
                if (e.button === 2 && isEditable(resolveTarget(e))) {
                    e.stopImmediatePropagation();
                }
            }, true);
            document.addEventListener('mouseup', function(e) {
                if (e.button === 2 && isEditable(resolveTarget(e))) {
                    e.stopImmediatePropagation();
                }
            }, true);

            // Interceptar evento paste para pegar imágenes desde el portapapeles del sistema
            document.addEventListener('paste', async function(e) {
                // Si el evento ya trae archivos, no interferir
                if (e.clipboardData && e.clipboardData.files && e.clipboardData.files.length > 0) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'info',
                        message: 'Paste con archivos del sistema (' + e.clipboardData.files.length + '), dejando pasar a WhatsApp'
                    });
                    return;
                }

                // Si WebKit ya expone la imagen en clipboardData.items, usarla directo
                const clipItems = e.clipboardData ? Array.from(e.clipboardData.items || []) : [];
                const imageItem = clipItems.find(function(it) { return it.type && it.type.startsWith('image/'); });
                if (imageItem) {
                    e.preventDefault();
                    e.stopImmediatePropagation();
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'info',
                        message: 'Imagen en clipboardData.items (' + imageItem.type + '), procesando directo'
                    });
                    var imgFile = imageItem.getAsFile();
                    if (imgFile) {
                        var imgDt = new DataTransfer();
                        imgDt.items.add(imgFile);
                        var imgPasteEvent;
                        try {
                            imgPasteEvent = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: imgDt });
                        } catch(_) {
                            imgPasteEvent = new Event('paste', { bubbles: true, cancelable: true });
                            Object.defineProperty(imgPasteEvent, 'clipboardData', { get: function() { return imgDt; }, configurable: true });
                        }
                        (document.activeElement || document.body).dispatchEvent(imgPasteEvent);
                    }
                    return;
                }

                const clipTypes = e.clipboardData ? Array.from(e.clipboardData.types || []) : [];

                // Solo text/plain o text/rtf indican un paste de texto real → dejar pasar a WhatsApp.
                // text/html sin text/plain puede ser una imagen envuelta en HTML
                // (ej: Firefox copia imagen → clipboard tiene image/png + text/html con <img>).
                // WebKit2GTK no expone el image/png externo en clipboardData.items, pero wl-paste sí
                // puede leerlo del clipboard del sistema → dejamos que caiga al path IPC.
                if (clipTypes.some(function(t) { return t === 'text/plain' || t === 'text/rtf'; })) {
                    return;
                }
                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'Paste sin texto plano. Tipos: ' + (clipTypes.join(', ') || '(vacío)') + '. Intentando IPC...'
                });

                // IMPORTANTE: detener el evento AHORA (sincrónico) para que WhatsApp no
                // procese el paste mientras esperamos la respuesta IPC (async).
                e.preventDefault();
                e.stopImmediatePropagation();

                // Obtener imagen del portapapeles via IPC
                let clipImage = null;
                if (window.__TAURI_INTERNALS__) {
                    try {
                        clipImage = await window.__TAURI_INTERNALS__.invoke('read_clipboard_image');
                    } catch(err) {
                        window.__TAURI_INTERNALS__.invoke('log_js', {
                            level: 'error',
                            message: 'Error al invocar read_clipboard_image: ' + (err.message || err)
                        });
                    }
                }
                if (!clipImage && window.__tauriClipboardImage) {
                    clipImage = { data: window.__tauriClipboardImage, mime_type: 'image/png' };
                }
                if (!clipImage) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'warn',
                        message: 'No hay imagen en el portapapeles (tipos JS: ' + (clipTypes.join(', ') || '(vacío)') + ')'
                    });
                    return;
                }

                // Convertir base64 a File
                const b64 = clipImage.data;
                const mimeType = clipImage.mime_type || 'image/png';
                const binary = atob(b64);
                const array = new Uint8Array(binary.length);
                for (let i = 0; i < binary.length; i++) array[i] = binary.charCodeAt(i);
                const blob = new Blob([array], { type: mimeType });
                const ext = mimeType.split('/')[1] || 'png';
                const file = new File([blob], 'clipboard.' + ext, { type: mimeType });

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

                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'DOM drop: files=' + (e.dataTransfer ? e.dataTransfer.files.length : 'N/A') +
                             ' types=[' + Array.from(e.dataTransfer ? e.dataTransfer.types || [] : []).join(',') + ']' +
                             ' uriList=' + (e.dataTransfer ? JSON.stringify(e.dataTransfer.getData('text/uri-list') || '') : 'N/A')
                });

                if (!e.dataTransfer) return;

                // Path 1: WebKit expone los archivos directamente en dataTransfer.files
                if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
                    const dt = new DataTransfer();
                    for (let i = 0; i < e.dataTransfer.files.length; i++) dt.items.add(e.dataTransfer.files[i]);
                    dispatchPasteWithFiles(dt);
                    return;
                }

                // Path 2: En Linux/WebKit2GTK los archivos no están en dataTransfer.files.
                // text/uri-list contiene las URIs de los archivos (file://) o URLs (https://).
                const rawUris = e.dataTransfer.getData('text/uri-list');
                if (!rawUris) return;
                const uris = rawUris.split(/\r?\n/).map(function(u) { return u.trim(); }).filter(function(u) { return u && !u.startsWith('#'); });
                if (!uris.length) return;

                window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                    level: 'info',
                    message: 'Drop via text/uri-list: ' + uris.join(', ')
                });

                (async function() {
                    const dt = new DataTransfer();
                    for (const uri of uris) {
                        try {
                            if (uri.startsWith('file://')) {
                                if (!window.__TAURI_INTERNALS__) continue;
                                const path = decodeURIComponent(new URL(uri).pathname);
                                const res = await window.__TAURI_INTERNALS__.invoke('read_file_for_drop', { path });
                                if (res) {
                                    const bin = atob(res.data), arr = new Uint8Array(bin.length);
                                    for (let i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
                                    dt.items.add(new File([new Blob([arr], { type: res.mime })], res.name, { type: res.mime }));
                                }
                            } else if (uri.startsWith('http://') || uri.startsWith('https://')) {
                                const resp = await fetch(uri);
                                if (!resp.ok) throw new Error('HTTP ' + resp.status);
                                const blob = await resp.blob();
                                const name = decodeURIComponent(uri.split('/').pop().split('?')[0]) || 'archivo';
                                const b64 = await new Promise(function(res, rej) {
                                    const r = new FileReader(); r.onload = function() { res(r.result.split(',')[1]); }; r.onerror = rej; r.readAsDataURL(blob);
                                });
                                const bin = atob(b64), arr = new Uint8Array(bin.length);
                                for (let i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
                                dt.items.add(new File([new Blob([arr], { type: blob.type || 'application/octet-stream' })], name, { type: blob.type || 'application/octet-stream' }));
                            }
                        } catch(err) {
                            window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                                level: 'error',
                                message: 'Drop: error con URI ' + uri + ': ' + (err.message || err)
                            });
                        }
                    }
                    dispatchPasteWithFiles(dt);
                })();
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
                let lastCount = -1;
                let titleObs = null;
                function checkCount() {
                    const title = document.title;
                    const m = title.match(/^\((\d+)\)/);
                    const count = m ? (parseInt(m[1], 10) || 0) : 0;
                    if (count !== lastCount) {
                        lastCount = count;
                        try { window.__TAURI_INTERNALS__.invoke('update_unread_count', { count: count }); } catch(e) {}
                    }
                }
                function attachTitleObserver() {
                    const titleEl = document.querySelector('title');
                    if (titleEl && !titleObs) {
                        titleObs = new MutationObserver(checkCount);
                        titleObs.observe(titleEl, { childList: true, characterData: true, subtree: true });
                        checkCount();
                    }
                }
                // Observe <head> for when WhatsApp inserts its <title>
                const headObs = new MutationObserver(attachTitleObserver);
                headObs.observe(document.documentElement, { childList: true, subtree: true });
                attachTitleObserver();
                setInterval(checkCount, 2500);
            })();

            // --- Drag & drop: prevent WebView from intercepting file drops as navigation ---
            document.addEventListener('dragover', function(e) { e.preventDefault(); }, false);

            // Bridge: Rust calls this after reading a dropped file (fallback when
            // WebKitGTK intercepts the native drop and tries to navigate to a file:// URL).
            // We dispatch a paste event (not a drop event) because WhatsApp Web accepts
            // synthetic ClipboardEvents but ignores synthetic DragEvents (isTrusted check).
            window.__tauriInjectDrop = function(fileName, base64Data, mimeType) {
                try {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'info',
                        message: '__tauriInjectDrop: ' + fileName + ' (' + (mimeType || 'octet-stream') + ')'
                    });
                    const binary = atob(base64Data);
                    const array = new Uint8Array(binary.length);
                    for (let i = 0; i < binary.length; i++) array[i] = binary.charCodeAt(i);
                    const blob = new Blob([array], { type: mimeType || 'application/octet-stream' });
                    const file = new File([blob], fileName, { type: mimeType || 'application/octet-stream' });
                    const dt = new DataTransfer();
                    dt.items.add(file);
                    dispatchPasteWithFiles(dt);
                } catch(e) {
                    window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke('log_js', {
                        level: 'error',
                        message: '__tauriInjectDrop error: ' + (e.message || e)
                    });
                }
            };
        })();
    "#)
    .build()?;

    // Enable spellcheck and devtools on linux webviews
    #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
    {
        let _ = window.with_webview(|w| {
            let webview = w.inner();
            if let Some(context) = webview.context() {
                context.set_spell_checking_enabled(true);
                context.set_spell_checking_languages(&["es_ES", "en_US"]);
            }
            if let Some(settings) = webview.settings() {
                settings.set_enable_developer_extras(true);
            }
        });
    }

    // Intercept file drops at the GTK widget level. On Linux/Wayland, WebKit2GTK
    // handles DnD internally before DOM events or Tauri's DragDropEvent can fire.
    // Connecting to drag-data-received on the underlying GtkWidget bypasses WebKit's
    // security sandbox and catches drops from Nautilus, browsers, and other apps.
    #[cfg(target_os = "linux")]
    {
        let app_dnd = window.app_handle().clone();
        let win_dnd = window.clone();
        let _ = window.with_webview(move |w| {
            use gtk::prelude::WidgetExt;

            let widget = w.inner();
            // Add text/uri-list as an accepted drop target (additive — does not
            // replace WebKit's own drag destination registration).
            widget.drag_dest_add_uri_targets();

            let app_h = app_dnd;
            let win_h = win_dnd;

            widget.connect_drag_data_received(move |_widget, _ctx, _x, _y, sel: &gtk::SelectionData, _info, _time| {
                let uris: Vec<glib::GString> = sel.uris();
                if uris.is_empty() { return; }

                let uri_strs: Vec<String> = uris.into_iter().map(|u| u.to_string()).collect();
                log_message(&app_h, LogLevel::Info,
                    format!("GTK drag-data-received: {} URI(s): {}", uri_strs.len(), uri_strs.join(", ")));

                let app_h2 = app_h.clone();
                let win_h2 = win_h.clone();

                std::thread::spawn(move || {
                    for uri in &uri_strs {
                        if !uri.starts_with("file://") { continue; }
                        let path = decode_file_uri(uri);
                        match std::fs::read(&path) {
                            Ok(data) => {
                                let name = path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("archivo")
                                    .to_string();
                                let mime = mime_guess::from_path(&path)
                                    .first_or_octet_stream()
                                    .to_string();
                                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                                log_message(&app_h2, LogLevel::Info,
                                    format!("GTK drop inyectado: '{}' ({})", name, mime));
                                let js = format!(
                                    "window.__tauriInjectDrop&&window.__tauriInjectDrop({},{},{})",
                                    serde_json::to_string(&name).unwrap(),
                                    serde_json::to_string(&b64).unwrap(),
                                    serde_json::to_string(&mime).unwrap(),
                                );
                                let _ = win_h2.eval(&js);
                            }
                            Err(e) => {
                                log_message(&app_h2, LogLevel::Error,
                                    format!("GTK drop: error leyendo '{}': {}", path.display(), e));
                            }
                        }
                    }
                });
            });
        });
    }

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
            log_message(&w.app_handle(), LogLevel::Info, format!(
                "DragDrop Tauri event: {} path(s)", paths.len()
            ));
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
                        let mime = mime_guess::from_path(path).first_or_octet_stream().to_string();
                        files_json.push_str(&format!(
                            r#"{{"name":"{}","data":"{}","mime":"{}"}}"#,
                            escaped_name, b64, mime
                        ));
                    }
                } else {
                    log_message(&w.app_handle(), LogLevel::Error, format!(
                        "No se pudo leer el archivo arrastrado: {}",
                        path.display()
                    ));
                }
            }
            files_json.push(']');

            let _ = w.eval(&format!(
                r#"(function(){{var files={};if(!files.length)return;var dt=new DataTransfer();for(var i=0;i<files.length;i++){{var f=files[i];var b=atob(f.data);var a=new Uint8Array(b.length);for(var j=0;j<b.length;j++)a[j]=b.charCodeAt(j);var blob=new Blob([a],{{type:f.mime||"application/octet-stream"}});dt.items.add(new File([blob],f.name,{{type:f.mime||"application/octet-stream"}}));}}if(typeof window.dispatchPasteWithFiles==='function'){{window.dispatchPasteWithFiles(dt);}}else{{console.error('[whataJOST] dispatchPasteWithFiles no disponible');}}}})()"#,
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

#[cfg(target_os = "linux")]
fn decode_file_uri(uri: &str) -> std::path::PathBuf {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    let bytes = path.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                decoded.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    std::path::PathBuf::from(String::from_utf8_lossy(&decoded).as_ref())
}

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
fn read_clipboard_image_raw(tool: &str) -> Option<(Vec<u8>, &'static str)> {
    const MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/bmp", "image/webp"];
    for &mime in MIME_TYPES {
        let output = match tool {
            "wl-paste" => std::process::Command::new("wl-paste")
                .args(["--type", mime, "--no-newline"])
                .output(),
            _ => std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", mime, "-o"])
                .output(),
        };
        if let Some(out) = output.ok().filter(|o| o.status.success() && !o.stdout.is_empty()) {
            return Some((out.stdout, mime));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn list_clipboard_types(tool: &str) -> String {
    let output = match tool {
        "wl-paste" => std::process::Command::new("wl-paste").args(["--list-types"]).output(),
        _ => std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
            .output(),
    };
    output
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            s.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "(no disponible)".to_string())
}

#[tauri::command]
fn read_file_for_drop(app: AppHandle, path: String) -> Option<FileDropResult> {
    // En Windows, URL.pathname devuelve /C:/... — strip del / inicial.
    #[cfg(target_os = "windows")]
    let path = if path.starts_with('/') { path[1..].to_string() } else { path };
    match std::fs::read(&path) {
        Ok(data) => {
            let name = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("archivo")
                .to_string();
            let mime = mime_guess::from_path(&path).first_or_octet_stream().to_string();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            log_message(&app, LogLevel::Info, format!("Drop: '{}' ({})", name, mime));
            Some(FileDropResult { name, data: b64, mime })
        }
        Err(e) => {
            log_message(&app, LogLevel::Error, format!("Drop: error leyendo '{}': {}", path, e));
            None
        }
    }
}

#[cfg(target_os = "linux")]
#[tauri::command]
fn read_clipboard_image(app: AppHandle) -> Option<ClipboardImage> {
    log_message(&app, LogLevel::Info, "Solicitud de lectura de portapapeles");
    if let Some(Some(tool)) = CLIPBOARD_TOOL.get() {
        match read_clipboard_image_raw(tool) {
            Some((data, mime)) => {
                log_message(&app, LogLevel::Info, format!("Imagen leída del portapapeles ({}, {})", tool, mime));
                return Some(ClipboardImage { data: base64_encode_bytes(&data), mime_type: mime.to_string() });
            }
            None => {
                let types = list_clipboard_types(tool);
                log_message(&app, LogLevel::Warn, format!(
                    "{} disponible pero no hay imagen en el portapapeles. Tipos disponibles: {}",
                    tool, types
                ));
                return None;
            }
        }
    }
    for tool in ["wl-paste", "xclip"] {
        match read_clipboard_image_raw(tool) {
            Some((data, mime)) => {
                let _ = CLIPBOARD_TOOL.set(Some(tool));
                log_message(&app, LogLevel::Info, format!("Herramienta detectada: {}. Imagen leída ({})", tool, mime));
                return Some(ClipboardImage { data: base64_encode_bytes(&data), mime_type: mime.to_string() });
            }
            None => {
                let types = list_clipboard_types(tool);
                log_message(&app, LogLevel::Info, format!(
                    "{}: no hay imagen en el portapapeles. Tipos disponibles: {}",
                    tool, types
                ));
            }
        }
    }
    let _ = CLIPBOARD_TOOL.set(None);
    log_message(&app, LogLevel::Warn, "No se encontró herramienta de portapapeles (wl-paste/xclip). Instalá wl-clipboard o xclip.");
    None
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
fn read_clipboard_image(_app: AppHandle) -> Option<ClipboardImage> {
    None
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
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

    if let Some(existing) = app.get_webview_window("notification") {
        let _ = existing.close();
    }

    let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let pos = monitor.position();
        let logical_w = size.width as f64 / scale;
        let logical_h = size.height as f64 / scale;
        (
            pos.x as f64 / scale + logical_w - 360.0 - 16.0,
            pos.y as f64 / scale + logical_h - 100.0 - 16.0,
        )
    } else {
        return;
    };

    let path = format!("notification.html?t={}&b={}", url_encode(&title), url_encode(&body));
    let result = WebviewWindowBuilder::new(&app, "notification", WebviewUrl::App(path.into()))
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
fn save_file(app: AppHandle, data: String, file_name: String) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| {
            let msg = format!("Error al decodificar archivo: {e}");
            log_message(&app, LogLevel::Error, &msg);
            msg
        })?;

    let path = rfd::FileDialog::new()
        .set_file_name(&file_name)
        .save_file();

    match path {
        Some(p) => {
            std::fs::write(&p, &bytes)
                .map_err(|e| {
                    let msg = format!("Error al guardar archivo: {e}");
                    log_message(&app, LogLevel::Error, &msg);
                    msg
                })?;
            let path_str = p.to_string_lossy().to_string();
            log_message(&app, LogLevel::Info, format!("Archivo guardado: {}", path_str));
            Ok(path_str)
        }
        None => {
            let dir = app.path().download_dir()
                .map_err(|e| {
                    let msg = format!("Error al obtener directorio de descargas: {e}");
                    log_message(&app, LogLevel::Error, &msg);
                    msg
                })?;
            let subdir = dir.join("WhataJOST");
            std::fs::create_dir_all(&subdir)
                .map_err(|e| {
                    let msg = format!("Error al crear directorio: {e}");
                    log_message(&app, LogLevel::Error, &msg);
                    msg
                })?;
            let fallback = subdir.join(&file_name);
            std::fs::write(&fallback, &bytes)
                .map_err(|e| {
                    let msg = format!("Error al guardar archivo: {e}");
                    log_message(&app, LogLevel::Error, &msg);
                    msg
                })?;
            let path_str = fallback.to_string_lossy().to_string();
            log_message(&app, LogLevel::Info, format!("Archivo guardado en fallback (sin diálogo): {}", path_str));
            Ok(path_str)
        }
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
        log_message(&app, LogLevel::Error, format!("Error al actualizar icono del tray: {e}"));
    }
}

#[tauri::command]
fn open_whatsapp(app: AppHandle) {
    show_window(&app);
    if let Some(win) = app.get_webview_window("notification") {
        let _ = win.close();
    }
}

#[tauri::command]
fn get_logs(state: State<'_, LogState>) -> Vec<LogEntry> {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn clear_logs(state: State<'_, LogState>) {
    state.0.lock().unwrap().clear();
}

#[tauri::command]
fn log_js(app: AppHandle, level: String, message: String) {
    let lvl = match level.as_str() {
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    };
    log_message(&app, lvl, message);
}

#[tauri::command]
fn open_logs(app: AppHandle) {
    if let Some(existing) = app.get_webview_window("logs") {
        let _ = existing.set_focus();
        return;
    }

    let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let pos = monitor.position();
        let logical_w = size.width as f64 / scale;
        let logical_h = size.height as f64 / scale;
        let win_w = 650.0f64;
        let win_h = 420.0f64;
        (
            pos.x as f64 / scale + (logical_w - win_w) / 2.0,
            pos.y as f64 / scale + (logical_h - win_h) / 2.0,
        )
    } else {
        (100.0f64, 100.0f64)
    };

    let result = WebviewWindowBuilder::new(
        &app,
        "logs",
        WebviewUrl::App("logs.html".into()),
    )
    .title("Logs - WhataJOST")
    .inner_size(650.0, 420.0)
    .position(x, y)
    .resizable(true)
    .build();

    if let Err(e) = result {
        log_message(&app, LogLevel::Error, format!("Error al abrir ventana de logs: {e}"));
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_window(app);
            if let Some(url) = args.iter().find(|a| a.starts_with("whatsapp://")) {
                handle_deep_link(app, url);
            }
        }))
        .invoke_handler(tauri::generate_handler![
            show_notification,
            close_notification,
            open_whatsapp,
            read_clipboard_image,
            read_file_for_drop,
            save_file,
            update_unread_count,
            get_logs,
            clear_logs,
            open_logs,
            log_js
        ])
        .setup(|app| {
            let notifications_enabled = load_notification_enabled(app.handle());
            app.manage(NotificationPopupState(Mutex::new(notifications_enabled)));
            app.manage(LogState(Arc::new(Mutex::new(Vec::new()))));
            log_message(app.handle(), LogLevel::Info, "WhataJOST iniciado");

            create_whatsapp_window(app.handle())?;

            // Manejar deep link si la app fue lanzada con una URL whatsapp://
            let launch_url = std::env::args()
                .skip(1)
                .find(|a| a.starts_with("whatsapp://"));
            if let Some(url) = launch_url {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    // Esperar a que WhatsApp Web cargue antes de navegar
                    std::thread::sleep(Duration::from_secs(5));
                    handle_deep_link(&handle, &url);
                });
            }

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
            let logs_item =
                MenuItem::with_id(app, "logs", "Ver logs", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&show_item, &autostart_item, &notify_item, &update_item, &logs_item, &quit_item],
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
                    "logs" => {
                        if let Some(existing) = app.get_webview_window("logs") {
                            let _ = existing.set_focus();
                        } else {
                            let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
                                let scale = monitor.scale_factor();
                                let size = monitor.size();
                                let pos = monitor.position();
                                let logical_w = size.width as f64 / scale;
                                let logical_h = size.height as f64 / scale;
                                (
                                    pos.x as f64 / scale + (logical_w - 650.0) / 2.0,
                                    pos.y as f64 / scale + (logical_h - 420.0) / 2.0,
                                )
                            } else {
                                (100.0f64, 100.0f64)
                            };
                            let result = WebviewWindowBuilder::new(
                                app,
                                "logs",
                                WebviewUrl::App("logs.html".into()),
                            )
                            .title("Logs - WhataJOST")
                            .inner_size(650.0, 420.0)
                            .position(x, y)
                            .resizable(true)
                            .build();
                            if let Err(e) = result {
                                log_message(app, LogLevel::Error, format!("Error al abrir ventana de logs: {e}"));
                            }
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

            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
                let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyW);
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(|app, _shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                if let Some(win) = app.get_webview_window("whatsapp-web") {
                                    if win.is_visible().unwrap_or(false) {
                                        let _ = win.hide();
                                    } else {
                                        show_window(app);
                                    }
                                }
                            }
                        })
                        .build()
                )?;
                match app.global_shortcut().register(shortcut) {
                    Ok(_) => log_message(app.handle(), LogLevel::Info, "Atajo registrado: Win+Alt+W"),
                    Err(e) => log_message(app.handle(), LogLevel::Warn, format!("No se pudo registrar atajo Win+Alt+W: {}", e)),
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
