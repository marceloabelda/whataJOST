use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

mod constants;
use constants::*;
mod logging;
mod window;
mod i18n;
mod resources;
mod updater;
mod clipboard;
mod config;
mod downloads;
mod tray;
mod notifications;
use logging::*;
use window::*;
use i18n::*;
use updater::*;
use clipboard::*;
use config::*;
use tray::*;
use notifications::*;

#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
use webkit2gtk::{SettingsExt, WebContextExt, WebViewExt};

use tauri::{
    image::Image,
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, DragDropEvent, Manager, State, WebviewEvent, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use base64::Engine;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_window(app);
            if let Some(url) = args.iter().find(|a| a.starts_with("whatsapp://")) {
                handle_deep_link(app, url);
            }
        }))
        .invoke_handler(tauri::generate_handler![
            notifications::show_notification,
            notifications::close_notification,
            window::open_whatsapp,
            clipboard::read_clipboard_image,
            clipboard::read_file_for_drop,
            downloads::save_file_begin,
            downloads::save_file_chunk,
            downloads::save_file_abort,
            downloads::save_file,
            tray::update_unread_count,
            logging::get_logs,
            logging::clear_logs,
            logging::open_logs,
            logging::log_js,
            resources::get_resource_usage
        ])
        .setup(|app| {
            let notifications_enabled = load_notification_enabled(app.handle());
            let account2_visible      = load_account2_visible(app.handle());
            let pdf_external_reader   = load_pdf_external_reader(app.handle());
            app.manage(NotificationPopupState(Mutex::new(notifications_enabled)));
            app.manage(Account2VisibleState(Mutex::new(account2_visible)));
            app.manage(PdfExternalReaderState(Mutex::new(pdf_external_reader)));
            app.manage(NotificationGenState(AtomicU64::new(0)));
            app.manage(LogState(Arc::new(Mutex::new(VecDeque::new()))));
            log_message(app.handle(), LogLevel::Info, "WhataJOST iniciado");

            create_whatsapp_window(app.handle())?;
            if account2_visible {
                // Diferido a un thread: crear la segunda ventana (nuevo contexto WebKit)
                // de forma sincrónica acá adentro de setup() retrasa que aparezcan el
                // tray y el atajo global hasta que termine de inicializarse.
                let handle2 = app.handle().clone();
                std::thread::spawn(move || {
                    if let Err(e) = create_whatsapp_window_2(&handle2) {
                        log_message(&handle2, LogLevel::Error, format!("Error al crear segunda cuenta al inicio: {e}"));
                    }
                });
            }

            // Manejar deep link si la app fue lanzada con una URL whatsapp://
            let launch_url = std::env::args()
                .skip(1)
                .find(|a| a.starts_with("whatsapp://"));
            if let Some(url) = launch_url {
                // La ventana ya existe; si WhatsApp todavía no cargó, el link
                // queda pendiente y lo navega on_page_load.
                handle_deep_link(app.handle(), &url);
            }

            let icon = Image::from_bytes(include_bytes!("../../public/tray.png"))?;

            app.manage(TrayBadgeState {
                base_rgba: icon.rgba().to_vec(),
                current_count: Mutex::new(0),
            });
            let menu = build_tray_menu(app.handle())?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("WhataJOST")
                .show_menu_on_left_click(false)
                .on_menu_event(|app: &AppHandle, event: MenuEvent| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => show_window(app),
                    "show2" => show_window_2(app),
                    "update" => check_for_updates(app),
                    "toggle_notify" => {
                        let state = app.state::<NotificationPopupState>();
                        let should_save = match state.0.lock() {
                            Ok(mut enabled) => {
                                *enabled = !*enabled;
                                Some(*enabled)
                            }
                            Err(_e) => {
                                eprintln!("[toggle_notify] NotificationPopupState poisoned");
                                None
                            }
                        };
                        if let Some(val) = should_save {
                            save_notification_enabled(app, val);
                        }
                    }
                    "account2" => {
                        let state = app.state::<Account2VisibleState>();
                        let new_visible = if let Ok(mut v) = state.0.lock() {
                            *v = !*v;
                            *v
                        } else {
                            eprintln!("[toggle_account2] Account2VisibleState poisoned");
                            false
                        };
                        save_account2_visible(app, new_visible);
                        if new_visible {
                            if app.get_webview_window("whatsapp-web-2").is_some() {
                                show_window_2(app);
                            } else if let Err(e) = create_whatsapp_window_2(app) {
                                log_message(app, LogLevel::Error, format!("Error al crear segunda cuenta: {e}"));
                            } else {
                                show_window_2(app);
                            }
                        } else if let Some(win) = app.get_webview_window("whatsapp-web-2") {
                            let _ = win.hide();
                        }
                        update_tray_menu(app);
                    }
                    "autostart" => {
                        let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                        if enabled {
                            let _ = app.autolaunch().disable();
                        } else {
                            let _ = app.autolaunch().enable();
                        }
                    }
                    "toggle_pdf_reader" => {
                        let state = app.state::<PdfExternalReaderState>();
                        let should_save = match state.0.lock() {
                            Ok(mut enabled) => {
                                *enabled = !*enabled;
                                Some(*enabled)
                            }
                            Err(_e) => {
                                eprintln!("[toggle_pdf_reader] PdfExternalReaderState poisoned");
                                None
                            }
                        };
                        if let Some(val) = should_save {
                            save_pdf_external_reader(app, val);
                        }
                    }
                    "logs" => open_logs(app.clone()),
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
                                show_window(app);
                            }
                        } else {
                            show_window(app);
                        }
                    }
                })
                .build(app)?;

            app.manage(tray);

            // Check for updates in background after app starts, then every 2 hours
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(FIRST_UPDATE_CHECK_DELAY_MS));
                loop {
                    check_for_updates_impl(&handle, true);
                    std::thread::sleep(Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS));
                }
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
