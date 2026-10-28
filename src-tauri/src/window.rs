use super::*;

/// Estado de carga de la ventana `whatsapp-web` y deep link pendiente.
/// Van bajo el mismo lock para que no haya carrera entre "terminó de cargar"
/// y "llegó un deep link": o lo navega `handle_deep_link` (ya cargó) o lo
/// toma `on_page_load` (todavía no).
pub(crate) static DEEP_LINK_STATE: Mutex<(bool, Option<String>)> = Mutex::new((false, None));

pub(crate) fn handle_deep_link(app: &AppHandle, url: &str) {
    // whatsapp://send?phone=5491112345678 → https://web.whatsapp.com/send?phone=5491112345678
    let web_url = format!(
        "https://web.whatsapp.com/{}",
        url.trim_start_matches("whatsapp://")
    );
    log_message(app, LogLevel::Info, format!("Deep link: {} → {}", url, web_url));
    show_window(app);
    let loaded = match DEEP_LINK_STATE.lock() {
        Ok(mut state) => {
            if !state.0 {
                state.1 = Some(web_url.clone());
            }
            state.0
        }
        Err(e) => {
            eprintln!("[deep_link] DEEP_LINK_STATE poisoned: {}", e);
            return;
        }
    };
    if loaded {
        navigate_whatsapp(app, &web_url);
    } else {
        log_message(app, LogLevel::Info, "Deep link en espera hasta que cargue WhatsApp Web");
    }
}

/// Llamado desde `on_page_load` cuando la ventana `whatsapp-web` termina de
/// cargar: marca la ventana como lista y navega el deep link pendiente, si hay.
pub(crate) fn on_whatsapp_loaded(app: &AppHandle) {
    let pending = match DEEP_LINK_STATE.lock() {
        Ok(mut state) => {
            state.0 = true;
            state.1.take()
        }
        Err(_) => None,
    };
    if let Some(url) = pending {
        navigate_whatsapp(app, &url);
    }
}

pub(crate) fn navigate_whatsapp(app: &AppHandle, url: &str) {
    let Some(win) = app.get_webview_window("whatsapp-web") else { return };
    match url.parse() {
        Ok(parsed) => {
            if let Err(e) = win.navigate(parsed) {
                log_message(app, LogLevel::Error, format!("Deep link: error al navegar: {e}"));
            }
        }
        Err(e) => log_message(app, LogLevel::Error, format!("Deep link: URL inválida {url}: {e}")),
    }
}

pub(crate) fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("whatsapp-web") {
        let _ = window.unminimize();
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        let _ = window.set_focus();
        let win = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(GTK_WINDOW_RESTORE_DELAY_MS));
            let _ = win.set_always_on_top(false);
        });
    } else {
        log_message(app, LogLevel::Error, "No se encontró la ventana de WhatsApp");
    }
}

pub(crate) fn show_window_2(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("whatsapp-web-2") {
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.set_always_on_top(false);
    }
}

pub(crate) fn create_whatsapp_window(app: &AppHandle) -> tauri::Result<()> {
    create_whatsapp_window_impl(app, "whatsapp-web", None)
}

pub(crate) fn create_whatsapp_window_2(app: &AppHandle) -> tauri::Result<()> {
    let data_dir = app.path().app_local_data_dir()
        .map(|p| p.join("account-2"))
        .ok();
    create_whatsapp_window_impl(app, "whatsapp-web-2", data_dir)
}

pub(crate) fn create_whatsapp_window_impl(app: &AppHandle, label: &str, data_dir: Option<std::path::PathBuf>) -> tauri::Result<()> {
    let opener = app.clone();
    let label_owned = label.to_string();
    let title = if label == "whatsapp-web" {
        format!("WhataJOST v{}", env!("CARGO_PKG_VERSION"))
    } else {
        format!("WhataJOST v{} — Cuenta 2", env!("CARGO_PKG_VERSION"))
    };
    let builder = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::External(
            WHATSAPP_WEB_URL
                .parse()
                .unwrap_or_else(|e| {
                    eprintln!("[init] Failed to parse WhatsApp URL: {}", e);
                    WHATSAPP_WEB_URL.parse().expect("hardcoded URL is valid")
                }),
        ),
    )
    .title(title)
    .inner_size(1280.0, 800.0)
    .visible(false)
    .focused(false)
    .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    .on_page_load(|win, payload| {
        if win.label() == "whatsapp-web" && payload.event() == tauri::webview::PageLoadEvent::Finished {
            on_whatsapp_loaded(win.app_handle());
        }
    })
    .on_navigation(move |url| {
        let scheme = url.scheme();
        if scheme == "data" || scheme == "about" {
            return true;
        }
        // Interceptar navegación a blob: — en lugar de navegar, disparar descarga via JS
        if scheme == "blob" {
            let url_str = url.as_str().to_string();
            if let Some(win) = opener.get_webview_window(&label_owned) {
                let safe = url_str.replace('\\', "\\\\").replace('\'', "\\'");
                let _ = win.eval(format!(
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
                serde_json::to_string(file_name).unwrap_or_else(|e| {
                    eprintln!("[paste] JSON serialization failed: {}", e);
                    "\"\"".to_string()
                }),
                serde_json::to_string(&b64).unwrap_or_else(|e| {
                    eprintln!("[paste] JSON serialization failed: {}", e);
                    "\"\"".to_string()
                }),
                serde_json::to_string(&mime).unwrap_or_else(|e| {
                    eprintln!("[paste] JSON serialization failed: {}", e);
                    "\"\"".to_string()
                }),
            );
            if let Some(win) = opener.get_webview_window(&label_owned) {
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
    .initialization_script(include_str!("inject.js"));
    let builder = if let Some(dir) = data_dir {
        builder.data_directory(dir)
    } else {
        builder
    };
    let window = builder.build()?;

    // Enable spellcheck and devtools on linux webviews
    #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
    {
        let _ = window.with_webview(|w| {
            let webview = w.inner();
            if let Some(context) = webview.context() {
                context.set_spell_checking_enabled(true);
                context.set_spell_checking_languages(&["es_ES", "en_US"]);
                // DocumentViewer desactiva la caché en memoria de recursos HTTP.
                // Medido: ~125 MB menos (~10%) sin diferencia de fluidez notable;
                // WhatsApp guarda sus datos en IndexedDB/service worker, no en esa caché.
                context.set_cache_model(webkit2gtk::CacheModel::DocumentViewer);
            }
            if let Some(settings) = webview.settings() {
                settings.set_enable_developer_extras(true);
            }
            // Sin esto, WebKit deniega por default cualquier permission-request
            // (navigator.clipboard.write, getUserMedia para llamadas, etc.) porque wry
            // no conecta esta señal. "Copiar imagen" en WhatsApp Web usa el Clipboard API
            // y fallaba silenciosamente (la imagen nunca llegaba al portapapeles del SO).
            use webkit2gtk::PermissionRequestExt;
            webview.connect_permission_request(|_wv, request| {
                request.allow();
                true
            });
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
                                    serde_json::to_string(&name).unwrap_or_else(|e| {
                                        eprintln!("[drag-drop] JSON serialization failed: {}", e);
                                        "\"\"".to_string()
                                    }),
                                    serde_json::to_string(&b64).unwrap_or_else(|e| {
                                        eprintln!("[drag-drop] JSON serialization failed: {}", e);
                                        "\"\"".to_string()
                                    }),
                                    serde_json::to_string(&mime).unwrap_or_else(|e| {
                                        eprintln!("[drag-drop] JSON serialization failed: {}", e);
                                        "\"\"".to_string()
                                    }),
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
            log_message(w.app_handle(), LogLevel::Info, format!(
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
                    log_message(w.app_handle(), LogLevel::Error, format!(
                        "No se pudo leer el archivo arrastrado: {}",
                        path.display()
                    ));
                }
            }
            files_json.push(']');

            let _ = w.eval(format!(
                r#"(function(){{var files={};if(!files.length)return;var dt=new DataTransfer();for(var i=0;i<files.length;i++){{var f=files[i];var b=atob(f.data);var a=new Uint8Array(b.length);for(var j=0;j<b.length;j++)a[j]=b.charCodeAt(j);var blob=new Blob([a],{{type:f.mime||"application/octet-stream"}});dt.items.add(new File([blob],f.name,{{type:f.mime||"application/octet-stream"}}));}}if(typeof window.dispatchPasteWithFiles==='function'){{window.dispatchPasteWithFiles(dt);}}else{{console.error('[whataJOST] dispatchPasteWithFiles no disponible');}}}})()"#,
                files_json
            ));
        }
    });

    Ok(())
}

/// Monitor donde ubicar ventanas auxiliares (toast, logs). En Wayland no hay
/// "monitor principal" y `primary_monitor()` devuelve None: se cae al monitor
/// de la ventana de WhatsApp, o al primero disponible.
pub(crate) fn target_monitor(app: &AppHandle) -> Option<tauri::Monitor> {
    app.primary_monitor().ok().flatten()
        .or_else(|| app.get_webview_window("whatsapp-web").and_then(|w| w.current_monitor().ok().flatten()))
        .or_else(|| app.available_monitors().ok().and_then(|m| m.into_iter().next()))
}

#[tauri::command]
pub(crate) fn open_whatsapp(app: AppHandle) {
    show_window(&app);
    if let Some(win) = app.get_webview_window("notification") {
        let _ = win.hide();
    }
}
