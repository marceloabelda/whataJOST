use super::*;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClipboardImage {
    pub(crate) data: String,
    pub(crate) mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FileDropResult {
    pub(crate) name: String,
    pub(crate) data: String,
    pub(crate) mime: String,
}

#[cfg(target_os = "linux")]
pub(crate) fn base64_encode_bytes(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
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
pub(crate) static CLIPBOARD_TOOL: OnceLock<Option<&'static str>> = OnceLock::new();

#[cfg(target_os = "linux")]
pub(crate) fn decode_file_uri(uri: &str) -> std::path::PathBuf {
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

#[cfg(target_os = "linux")]
pub(crate) fn read_clipboard_image_raw(tool: &str) -> Option<(Vec<u8>, &'static str)> {
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
pub(crate) fn list_clipboard_types(tool: &str) -> String {
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
pub(crate) fn read_file_for_drop(app: AppHandle, path: String) -> Option<FileDropResult> {
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
pub(crate) fn read_clipboard_image(app: AppHandle) -> Option<ClipboardImage> {
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
pub(crate) fn read_clipboard_image(_app: AppHandle) -> Option<ClipboardImage> {
    None
}
