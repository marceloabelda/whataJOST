use super::*;

pub(crate) fn whatajost_downloads_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("Error al obtener directorio de descargas: {e}"))?
        .join("WhataJOST");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Error al crear directorio: {e}"))?;
    Ok(dir)
}

pub(crate) fn unique_path(dir: &std::path::Path, file_name: &str) -> std::path::PathBuf {
    let mut path = dir.join(file_name);
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned());
    let mut n = 1;
    loop {
        let candidate_name = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            path = candidate;
            break;
        }
        n += 1;
    }
    path
}

// --- tray badge rendering ---

pub(crate) static SAVE_UPLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Archivo temporal donde se van juntando las partes de una descarga.
/// Va en el directorio de caché de la app (mismo filesystem que el home en
/// el caso típico, así el `rename` final no tiene que copiar).
pub(crate) fn save_upload_path(app: &AppHandle, id: u64) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Error al obtener directorio de caché: {e}"))?
        .join("descargas");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Error al crear directorio temporal: {e}"))?;
    Ok(dir.join(format!("{id}.part")))
}

/// Mueve el archivo temporal a su destino; si `rename` falla (otro
/// filesystem), copia y borra el original.
pub(crate) fn move_upload(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    if std::fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dest)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

#[tauri::command]
pub(crate) fn save_file_begin(app: AppHandle) -> Result<u64, String> {
    let id = SAVE_UPLOAD_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = save_upload_path(&app, id)?;
    // Primera descarga de la sesión: limpiar restos de sesiones anteriores
    // (la app pudo cerrarse a mitad de una descarga).
    if id == 0 {
        if let Some(dir) = path.parent() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    std::fs::File::create(&path).map_err(|e| {
        let msg = format!("Error al crear archivo temporal: {e}");
        log_message(&app, LogLevel::Error, &msg);
        msg
    })?;
    Ok(id)
}

// async: corre fuera del hilo principal, así decodificar y escribir partes
// grandes no congela la UI.
#[tauri::command]
pub(crate) async fn save_file_chunk(app: AppHandle, id: u64, data: String) -> Result<(), String> {
    use std::io::Write;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Error al decodificar archivo: {e}"))?;
    let path = save_upload_path(&app, id)?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(&bytes))
        .map_err(|e| {
            let msg = format!("Error al escribir archivo temporal: {e}");
            log_message(&app, LogLevel::Error, &msg);
            msg
        })
}

#[tauri::command]
pub(crate) fn save_file_abort(app: AppHandle, id: u64) {
    if let Ok(path) = save_upload_path(&app, id) {
        let _ = std::fs::remove_file(path);
    }
}

#[tauri::command]
pub(crate) fn save_file(app: AppHandle, id: u64, file_name: String) -> Result<String, String> {
    let tmp = save_upload_path(&app, id)?;
    let safe_name = sanitize_file_name(&file_name);
    if safe_name != file_name {
        log_message(&app, LogLevel::Warn, format!("Nombre de archivo saneado: {:?} → {:?}", file_name, safe_name));
    }
    let result = save_file_impl(&app, &tmp, &safe_name);
    // Si se movió ya no existe; si hubo error o cancelación, no dejar basura.
    let _ = std::fs::remove_file(&tmp);
    result
}

/// El nombre viene de la página (atributo `download`, normalmente el nombre que
/// puso quien mandó el archivo). Se usa con `dir.join(...)` en el fallback y en
/// el modo PDF externo: un nombre con `../` o absoluto escribiría fuera de la
/// carpeta de descargas (ej. `~/.config/autostart/`). Nos quedamos solo con el
/// nombre base y reemplazamos caracteres inválidos en Windows.
pub(crate) fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "archivo".to_string()
    } else {
        cleaned.to_string()
    }
}

pub(crate) fn save_file_impl(app: &AppHandle, tmp: &std::path::Path, file_name: &str) -> Result<String, String> {
    let pdf_external = file_name.to_lowercase().ends_with(".pdf")
        && app.state::<PdfExternalReaderState>()
            .0
            .lock()
            .map(|s| *s)
            .unwrap_or(false);

    if pdf_external {
        let dir = whatajost_downloads_dir(app).inspect_err(|e| {
            log_message(app, LogLevel::Error, e);
        })?;
        let path = unique_path(&dir, file_name);
        move_upload(tmp, &path).map_err(|e| {
            let msg = format!("Error al guardar PDF: {e}");
            log_message(app, LogLevel::Error, &msg);
            msg
        })?;
        let path_str = path.to_string_lossy().to_string();
        log_message(app, LogLevel::Info, format!("PDF guardado en {}, abriendo con lector externo", path_str));
        if let Err(e) = app.opener().open_path(&path_str, None::<&str>) {
            log_message(app, LogLevel::Warn, format!("opener.open_path falló ({}), usando xdg-open...", e));
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open")
                .arg(&path_str)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        return Ok(path_str);
    }

    let path = rfd::FileDialog::new()
        .set_file_name(file_name)
        .save_file();

    match path {
        Some(p) => {
            move_upload(tmp, &p)
                .map_err(|e| {
                    let msg = format!("Error al guardar archivo: {e}");
                    log_message(app, LogLevel::Error, &msg);
                    msg
                })?;
            let path_str = p.to_string_lossy().to_string();
            log_message(app, LogLevel::Info, format!("Archivo guardado: {}", path_str));
            Ok(path_str)
        }
        None => {
            let subdir = whatajost_downloads_dir(app).inspect_err(|e| {
                log_message(app, LogLevel::Error, e);
            })?;
            let fallback = subdir.join(file_name);
            move_upload(tmp, &fallback)
                .map_err(|e| {
                    let msg = format!("Error al guardar archivo: {e}");
                    log_message(app, LogLevel::Error, &msg);
                    msg
                })?;
            let path_str = fallback.to_string_lossy().to_string();
            log_message(app, LogLevel::Info, format!("Archivo guardado en fallback (sin diálogo): {}", path_str));
            Ok(path_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_file_name;

    #[test]
    fn nombres_normales_no_cambian() {
        assert_eq!(sanitize_file_name("foto.jpg"), "foto.jpg");
        assert_eq!(sanitize_file_name("Informe final (v2).pdf"), "Informe final (v2).pdf");
        assert_eq!(sanitize_file_name(".bashrc"), ".bashrc");
    }

    #[test]
    fn path_traversal_queda_en_el_nombre_base() {
        assert_eq!(sanitize_file_name("../../.config/autostart/x.desktop"), "x.desktop");
        assert_eq!(sanitize_file_name("/home/user/.bashrc"), ".bashrc");
        assert_eq!(sanitize_file_name("..\\..\\Windows\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_file_name("C:\\Users\\x\\a.txt"), "a.txt");
    }

    #[test]
    fn nombres_vacios_o_especiales() {
        assert_eq!(sanitize_file_name(""), "archivo");
        assert_eq!(sanitize_file_name(".."), "archivo");
        assert_eq!(sanitize_file_name("../"), "archivo");
        assert_eq!(sanitize_file_name("   "), "archivo");
    }

    #[test]
    fn caracteres_invalidos_en_windows() {
        assert_eq!(sanitize_file_name("a:b.txt"), "a_b.txt");
        assert_eq!(sanitize_file_name("que?.pdf"), "que_.pdf");
        assert_eq!(sanitize_file_name("x\u{0}y.txt"), "x_y.txt");
    }
}
