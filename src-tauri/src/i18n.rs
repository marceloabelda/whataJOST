
pub(crate) fn ui_lang_es() -> bool {
    #[cfg(target_os = "linux")]
    let locale = glib::language_names().first().map(|s| s.to_string()).unwrap_or_default();
    #[cfg(not(target_os = "linux"))]
    let locale = std::env::var("LANG").unwrap_or_default();
    locale.to_lowercase().starts_with("es")
}

pub(crate) fn t(key: &'static str) -> &'static str {
    let es = ui_lang_es();
    match (key, es) {
        ("tray_open", true) => "Abrir WhatsApp",
        ("tray_open", false) => "Open WhatsApp",
        ("tray_open2", true) => "Abrir WhatsApp (2da cuenta)",
        ("tray_open2", false) => "Open WhatsApp (2nd account)",
        ("tray_autostart", true) => "Iniciar con el sistema",
        ("tray_autostart", false) => "Start with system",
        ("tray_notify", true) => "Notificaciones emergentes",
        ("tray_notify", false) => "Popup notifications",
        ("tray_account2", true) => "Segunda cuenta WhatsApp",
        ("tray_account2", false) => "Second WhatsApp account",
        ("tray_pdf_reader", true) => "Descargar PDF abre con visor externo",
        ("tray_pdf_reader", false) => "Downloaded PDFs open in external viewer",
        ("tray_update", true) => "Buscar actualización",
        ("tray_update", false) => "Check for updates",
        ("tray_logs", true) => "Ver logs",
        ("tray_logs", false) => "View logs",
        ("tray_about", true) => "Acerca de",
        ("tray_about", false) => "About",
        ("tray_quit", true) => "Salir",
        ("tray_quit", false) => "Quit",
        ("dlg_title_update", true) => "whataJOST - Actualización",
        ("dlg_title_update", false) => "whataJOST - Update",
        ("dlg_title_error", true) => "whataJOST - Error",
        ("dlg_title_error", false) => "whataJOST - Error",
        _ => key,
    }
}
