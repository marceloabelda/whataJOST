use super::*;

pub(crate) struct NotificationPopupState(pub(crate) Mutex<bool>);
pub(crate) struct Account2VisibleState(pub(crate) Mutex<bool>);
pub(crate) struct PdfExternalReaderState(pub(crate) Mutex<bool>);
pub(crate) fn config_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = app.path().app_config_dir().expect("failed to get config dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("config.json")
}

pub(crate) fn load_notification_enabled(app: &AppHandle) -> bool {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("notifications_enabled").and_then(|e| e.as_bool()))
        .unwrap_or(true)
}

pub(crate) fn modify_config(app: &AppHandle, f: impl FnOnce(&mut serde_json::Value)) {
    let path = config_path(app);
    let mut json: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    f(&mut json);
    if let Err(e) = std::fs::write(&path, json.to_string()) {
        log_message(app, LogLevel::Error, format!("Error al guardar config: {e}"));
    }
}

pub(crate) fn save_notification_enabled(app: &AppHandle, enabled: bool) {
    modify_config(app, |json| {
        json["notifications_enabled"] = serde_json::Value::Bool(enabled);
    });
}

pub(crate) fn load_account2_visible(app: &AppHandle) -> bool {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("account2_visible").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

pub(crate) fn save_account2_visible(app: &AppHandle, visible: bool) {
    modify_config(app, |json| {
        json["account2_visible"] = serde_json::Value::Bool(visible);
    });
}

pub(crate) fn load_pdf_external_reader(app: &AppHandle) -> bool {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("pdf_external_reader").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

pub(crate) fn save_pdf_external_reader(app: &AppHandle, enabled: bool) {
    modify_config(app, |json| {
        json["pdf_external_reader"] = serde_json::Value::Bool(enabled);
    });
}
