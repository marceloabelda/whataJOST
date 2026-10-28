use super::*;

pub(crate) fn check_for_updates(app: &AppHandle) {
    check_for_updates_impl(app, false);
}

pub(crate) fn check_for_updates_impl(app: &AppHandle, silent: bool) {
    let handle = app.clone();
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async {
            let updater = match handle.updater() {
                Ok(u) => u,
                Err(e) => {
                    if !silent {
                        let es = ui_lang_es();
                        handle
                            .dialog()
                            .message(if es { format!("Error al iniciar el updater: {e}") } else { format!("Failed to start the updater: {e}") })
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
                        let es = ui_lang_es();
                        handle
                            .dialog()
                            .message(if es { "Ya tenés la última versión." } else { "You already have the latest version." })
                            .title("whataJOST")
                            .kind(MessageDialogKind::Info)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    return;
                }
                Err(e) => {
                    if !silent {
                        let es = ui_lang_es();
                        handle
                            .dialog()
                            .message(if es { format!("Error al buscar actualizaciones: {e}") } else { format!("Failed to check for updates: {e}") })
                            .title("whataJOST")
                            .kind(MessageDialogKind::Error)
                            .buttons(MessageDialogButtons::Ok)
                            .show(|_| {});
                    }
                    return;
                }
            };

            let version = update.version.clone();
            let es = ui_lang_es();

            handle
                .dialog()
                .message(if es {
                    format!("Nueva versión disponible: {version}\n\n¿Querés actualizar ahora?")
                } else {
                    format!("New version available: {version}\n\nDo you want to update now?")
                })
                .title(t("dlg_title_update"))
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
                                        .message(if es { format!("Error al descargar: {e}") } else { format!("Failed to download: {e}") })
                                        .title(t("dlg_title_error"))
                                        .kind(MessageDialogKind::Error)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                    return;
                                }
                            };

                            // AppImage no requiere privilegios de root: reemplazar el propio
                            // archivo en su lugar (vía el installer del plugin, que hace un
                            // rename→write→restore-permissions seguro) y relanzar alcanza. La
                            // cascada pkexec/dpkg de abajo es solo para instalaciones .deb.
                            #[cfg(target_os = "linux")]
                            if std::env::var_os("APPIMAGE").is_some() {
                                match update.install(bytes) {
                                    Ok(()) => {
                                        log_message(&h, LogLevel::Info, "AppImage actualizado in situ. Relanzando...");
                                        h.restart();
                                    }
                                    Err(e) => {
                                        h.dialog()
                                            .message(if es { format!("Error al instalar: {e}") } else { format!("Failed to install: {e}") })
                                            .title(t("dlg_title_error"))
                                            .kind(MessageDialogKind::Error)
                                            .buttons(MessageDialogButtons::Ok)
                                            .show(|_| {});
                                    }
                                }
                                return;
                            }

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
                                    .message(if es { format!("Error al guardar el paquete: {e}") } else { format!("Failed to save the package: {e}") })
                                    .title(t("dlg_title_error"))
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
                                    let password_title = if es { "whataJOST - Instalar actualización" } else { "whataJOST - Install update" };
                                    let password = std::process::Command::new("zenity")
                                        .args(["--password", "--title", password_title])
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
                                                if let Some(ref mut stdin) = child.stdin {
                                                    let _ = stdin.write_all(pass.as_bytes());
                                                }
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
                                        .message(if es {
                                            format!(
                                                "Se abrió el instalador del sistema con el paquete.\nCompletá la instalación manualmente y reiniciá whataJOST.\n\nArchivo: {}",
                                                tmp.display()
                                            )
                                        } else {
                                            format!(
                                                "The system installer was opened with the package.\nComplete the installation manually and restart whataJOST.\n\nFile: {}",
                                                tmp.display()
                                            )
                                        })
                                        .title(t("dlg_title_update"))
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
                                        .message(if es {
                                            format!("La instalación terminó con código: {}", status.code().unwrap_or(-1))
                                        } else {
                                            format!("The installation finished with code: {}", status.code().unwrap_or(-1))
                                        })
                                        .title(t("dlg_title_error"))
                                        .kind(MessageDialogKind::Error)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                }
                                Err(e) => {
                                    h.dialog()
                                        .message(if es { format!("Error al instalar: {e}") } else { format!("Failed to install: {e}") })
                                        .title(t("dlg_title_error"))
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
