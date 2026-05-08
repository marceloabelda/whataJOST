# WhataJOST

WhatsApp Web wrapper built with Tauri v2 (Rust backend + WebKitGTK webview).

## Workflow

Para cada tarea, seguí estos pasos en orden:

1. **Buscar** — Usá `mcp__codebase-search__search_codebase` (si está disponible), más la documentación del proyecto y los archivos fuente relevantes para entender el contexto antes de escribir código.
2. **Codificar** — Implementá los cambios necesarios.
3. **Documentar** — Mantené actualizado este CLAUDE.md si cambiaron decisiones de arquitectura, flujos, o dependencias.
4. **Commit y push** — Hacé commit con mensaje descriptivo en español y pusheá con `git push origin main` (empuja a GitHub y git.jost.ar en simultáneo). El pull solo se hace desde GitHub.

## Architecture

- `src-tauri/src/lib.rs` — Lógica principal: ventana WhatsApp, script inyectado, comandos Tauri, bandeja
- `src-tauri/Cargo.toml` — Dependencias Rust
- `src-tauri/capabilities/` — Permisos Tauri por ventana
- `src-tauri/tauri.conf.json` — Configuración de Tauri
- `latest.json` — Metadata del auto-updater

### Injected script

La ventana `whatsapp-web` carga `https://web.whatsapp.com/` con un script de inicialización inyectado que:
- Intercepta clicks en `<a download>` y `<a blob:>` para guardar archivos via `save_file`
- Rastrea blobs con `URL.createObjectURL`/`revokeObjectURL`
- Sobreescribe `HTMLAnchorElement.prototype.click` para capturar descargas programáticas
- Intercepta `paste` para pegar imágenes del portapapeles
- Intercepta `drag & drop` y `window.open` para links externos
- Permite menú contextual nativo en campos editables para corrección ortográfica
- Monitorea `document.title` para badge de mensajes no leídos

### Comandos Tauri

- `save_file(data, file_name)` — Abre diálogo nativo `rfd::FileDialog` y guarda archivo
- `update_unread_count(count)` — Actualiza badge en el ícono de bandeja
- `read_clipboard_image` — Lee imagen del portapapeles (Linux)
- `show_notification(title, body)` — Muestra notificación toast nativa

## Key dependencies

- `rfd` v0.16 con backend `xdg-portal` + `wayland` + `tokio` para diálogos de archivo
- `tauri-plugin-dialog` con feature `xdg-portal` para diálogos de mensaje
- `webkit2gtk` (Linux) para habilitar corrección ortográfica

## Release

Para publicar una nueva versión:

```bash
./release.sh <version>
# Ejemplo: ./release.sh 1.1.34
```

El script:
1. Actualiza la versión en `package.json`, `src-tauri/Cargo.toml` y `src-tauri/tauri.conf.json`
2. Commitea los cambios de versión
3. Crea y pushea el tag `v<version>`
4. Ejecuta `cargo update` y `pnpm update`
5. Buildea con `npx tauri build`
6. Genera `latest.json` con las firmas de los bundles
7. Crea la release en GitHub con `gh release create` (incluye `.deb`, `.sig` y `latest.json`)

Requisitos:
- Estar en la rama `main` sin cambios pendientes
- Tener `gh` CLI instalado
- Tener la clave privada del updater en `src-tauri/updater_key`
