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
        })();
    "#)
    .build()?;

    let win = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win.hide();
        }
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .invoke_handler(tauri::generate_handler![])
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
