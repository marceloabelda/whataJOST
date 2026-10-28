use super::*;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LogEntry {
    pub(crate) timestamp: String,
    pub(crate) level: LogLevel,
    pub(crate) message: String,
}

pub(crate) struct LogState(pub(crate) Arc<Mutex<VecDeque<LogEntry>>>);

pub(crate) fn format_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

pub(crate) fn log_message(app: &AppHandle, level: LogLevel, message: impl Into<String>) {
    let state = app.state::<LogState>();
    let result = state.0.lock();
    match result {
        Ok(mut logs) => {
            logs.push_back(LogEntry {
                timestamp: format_timestamp(),
                level,
                message: message.into(),
            });
            while logs.len() > MAX_LOG_ENTRIES {
                logs.pop_front();
            }
        }
        Err(e) => {
            eprintln!("[LogState] Mutex poisoned: {}", e);
        }
    }
}

#[tauri::command]
pub(crate) fn get_logs(state: State<'_, LogState>) -> Vec<LogEntry> {
    state
        .0
        .lock()
        .map(|logs| logs.iter().cloned().collect())
        .unwrap_or_else(|e| {
            eprintln!("[get_logs] LogState poisoned: {}", e);
            Vec::new()
        })
}

#[tauri::command]
pub(crate) fn clear_logs(state: State<'_, LogState>) {
    match state.0.lock() {
        Ok(mut logs) => logs.clear(),
        Err(e) => eprintln!("[clear_logs] LogState poisoned: {}", e),
    }
}

#[tauri::command]
pub(crate) fn log_js(app: AppHandle, level: String, message: String) {
    let lvl = match level.as_str() {
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    };
    log_message(&app, lvl, message);
}

#[tauri::command]
pub(crate) fn open_logs(app: AppHandle) {
    if let Some(existing) = app.get_webview_window("logs") {
        let _ = existing.set_focus();
        return;
    }

    let (x, y) = if let Some(monitor) = target_monitor(&app) {
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
    .inner_size(720.0, 540.0)
    .position(x, y)
    .resizable(true)
    .build();

    if let Err(e) = result {
        log_message(&app, LogLevel::Error, format!("Error al abrir ventana de logs: {e}"));
    }
}
