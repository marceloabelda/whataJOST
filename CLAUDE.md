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

### Script inyectado (`initialization_script` en `create_whatsapp_window`)

La ventana `whatsapp-web` carga `https://web.whatsapp.com/` con un script inyectado antes de que cargue la página. El script:

- **Descargas** — flujo completo en tres capas:

  **Cómo genera WhatsApp Web una descarga:**
  1. Crea el contenido como `Blob` y obtiene una `blob:` URL con `URL.createObjectURL(blob)`.
  2. Crea un `<a href="blob:..." download="nombre.ext">` y llama `.click()` programáticamente (o el usuario hace clic directo).

  **Tres puntos de intercepción (en orden de prioridad):**

  1. **`HTMLAnchorElement.prototype.click`** (lib.rs:481) — cacha `.click()` programáticos. Si `this.href` empieza con `blob:`, desvía a `downloadBlob(url, fileName)` y retorna sin propagar.
  2. **`document.addEventListener('click', ..., true)`** (lib.rs:496) — capture phase, cacha clics del usuario sobre `<a href="blob:...">` ya en el DOM. Llama `preventDefault` + `stopImmediatePropagation` y desvía a `downloadBlob`.
  3. **`on_navigation` (Rust, lib.rs:320)** — si el webview intenta navegar a una URL `blob:` (ninguno de los anteriores la capturó), Rust cancela la navegación y llama `window.__waDownloadBlob(url)` vía `win.eval(...)`.

  Adicionalmente, **`window.open(blob:...)`** (lib.rs:390) también se intercepta y desvía a `downloadBlob` (algunos flujos de exportación de WhatsApp usan `window.open` en lugar de un `<a>`).

  **`blobStore`** (lib.rs:409): sobrescribe `URL.createObjectURL` para guardar cada `blob:URL → Blob` en un `Map`. Sobrescribe `URL.revokeObjectURL` para eliminar la entrada recién 60 segundos después (no inmediatamente), evitando que `downloadBlob` falle si WhatsApp ya revocó la URL antes de que `FileReader` termine de leerla.

  **`downloadBlob(url, fileName)`** (lib.rs:444):
  1. Busca el blob en `blobStore`. Si no está (fue revocado hace >60 s o no pasó por el store), hace `fetch(url)`.
  2. Llama `saveBlob(blob, fileName)`.

  **`saveBlob(blob, fileName)`** (lib.rs:423):
  1. Convierte el blob a base64 con `FileReader.readAsDataURL`.
  2. Invoca IPC `save_file(data, fileName)` → Rust.

  **`save_file` (Rust, lib.rs:1212)**:
  1. Decodifica base64.
  2. Abre `rfd::FileDialog::new().set_file_name(name).save_file()`.
  3. Si el diálogo no aparece (sin xdg-portal activo) o el usuario cancela: guarda en `~/Downloads/WhataJOST/<fileName>` como fallback.
- **Paste de imágenes** — handler capture-phase en `document` (lib.rs:560), flujo completo:

  **Cuatro ramas en orden:**

  1. **`clipboardData.files.length > 0`** → el paste ya trae archivos (e.g., arrastrado desde el SO o paste sintético que generamos nosotros mismos); dejar pasar a WhatsApp sin tocar nada.

  2. **`clipboardData.items` tiene un item `image/*`** → WebKit2GTK expone la imagen directamente (ocurre cuando la imagen fue copiada dentro del mismo WebView). Se llama `imageItem.getAsFile()`, se agrega el `File` a un `DataTransfer` nuevo y se despacha un `ClipboardEvent` sintético con `clipboardData: dt` al elemento activo. Ese evento sintético vuelve a pasar por este handler, pero cae en la rama 1 (`files.length > 0`) y llega a WhatsApp.

  3. **`clipboardData.types` tiene `text/plain` o `text/rtf`** → es texto real, dejar pasar a WhatsApp. **No** se incluye `text/html` en esta condición: Firefox/Chrome al copiar una imagen agregan un `<img>` en `text/html` como fallback + `image/png` en el clipboard del sistema, pero WebKit2GTK no expone el `image/png` externo en `clipboardData.items`. Si se pasara a WhatsApp, éste recibiría el HTML pero no la imagen → nada útil.

  4. **Sin texto plano / sin archivos / sin imagen en items** (incluye `text/html` solo, clipboard vacío, imagen de app externa): el evento se para con `stopImmediatePropagation()` + `preventDefault()` **sincrónicamente** (antes de cualquier `await`; si se esperara, WhatsApp procesaría el paste vacío mientras llega la respuesta IPC). Luego:
     - Invoca `read_clipboard_image` (IPC → `wl-paste --type image/png|jpeg|bmp|webp` o `xclip`, detectando la herramienta con `OnceLock`).
     - Si devuelve `null` → log de warning, no se pega nada.
     - Si devuelve `{data: base64, mime_type}` → decodifica el base64 a `Uint8Array`, crea un `Blob` y un `File('clipboard.ext')`, agrega a `DataTransfer` y despacha `ClipboardEvent` sintético al elemento activo → cae en rama 1 → WhatsApp lo recibe como imagen.
- **Drag & drop**: intercepta `dragover`/`drop` y despacha paste sintético; `on_navigation` lee archivos `file://` arrastrados desde el SO y los inyecta vía `window.__tauriInjectDrop`
- **Links externos**: intercepta `window.open` y clicks en `<a>` no-WhatsApp para abrir en el browser del SO
- **Menú contextual**: permite el menú nativo de WebKitGTK en campos editables (corrección ortográfica) bloqueando los listeners de WhatsApp en capture phase
- **Badge no leídos**: observa `document.title` con `MutationObserver` + polling y llama `update_unread_count`
- **Notificaciones**: reemplaza `window.Notification` con invocaciones a `show_notification` (IPC)

### Comandos Tauri (IPC)

| Comando | Descripción |
|---|---|
| `save_file(data, file_name)` | Abre `rfd::FileDialog`; si el diálogo no aparece (xdg-portal), guarda en `~/Downloads/WhataJOST/` y lo registra en logs |
| `read_clipboard_image` | Lee imagen del portapapeles del sistema con `wl-paste` o `xclip` (detecta herramienta disponible con `OnceLock`) |
| `update_unread_count(count)` | Renderiza badge numérico sobre el ícono de bandeja (pixel art en RGBA, sin librerías de imágenes) |
| `show_notification(title, body)` | Abre ventana `notification` (HTML transparente, always-on-top) que se auto-cierra a los 5,5 s |
| `close_notification` | Cierra la ventana de notificación |
| `open_whatsapp` | Muestra y enfoca la ventana principal |
| `log_js(level, message)` | Recibe logs del script JS y los agrega al buffer en memoria |
| `get_logs` | Devuelve todos los logs del buffer (usada por `logs.html`) |
| `clear_logs` | Vacía el buffer de logs |
| `open_logs` | Abre la ventana del visor de logs centrada en pantalla |

### Capabilities (IPC por ventana)

| Archivo | Ventana | Permisos |
|---|---|---|
| `default.json` | `main` (oculta, 1×1 px) | `core`, `opener`, `updater`, `dialog`, `autostart`, `allow-whatsapp-ipc` |
| `whatsapp-notification.json` | `whatsapp-web` (remota: `*.whatsapp.com`, `*.whatsapp.net`) | `core:default`, `clipboard-manager:allow-read-image`, `allow-whatsapp-ipc` |
| `logs-window.json` | `logs` | `core:default`, `allow-whatsapp-ipc` |
| `notification-window.json` | `notification` | `core:default`, `allow-whatsapp-ipc` |

La ventana `whatsapp-web` carga una URL externa; el IPC solo funciona cuando la URL coincide con los patrones de `remote.urls`.

**Importante — ACL de Tauri 2.11.1+**: Desde Tauri 2.11.1, todos los comandos custom de la app (no solo plugins) requieren permiso ACL explícito cuando son invocados desde webviews remotos. El archivo `src-tauri/permissions/default.toml` define el permiso `allow-whatsapp-ipc` que lista los 10 comandos IPC de la app. Sin ese archivo (o sin incluir `allow-whatsapp-ipc` en el capability de la ventana remota), los comandos fallan silenciosamente — no aparece ningún error en la consola JS ni en los logs de Rust.

### Ventanas HTML (`public/`)

- `logs.html` — Visor de logs en tiempo real (polling 2 s a `get_logs`); muestra error explícito si IPC no está disponible
- `notification.html` — Toast de notificación transparente (se auto-cierra)
- `index.html` — Página mínima para la ventana `main` (oculta)

## Key dependencies

- `rfd` v0.16 con features `xdg-portal` + `wayland` + `tokio` — diálogos de archivo
- `tauri-plugin-dialog` con feature `xdg-portal` — diálogos de mensaje nativos (updater, errores)
- `webkit2gtk` (Linux) — corrección ortográfica + devtools en producción
- `mime_guess` — detección de MIME type para archivos arrastrados desde el SO
- `wl-clipboard` (`wl-paste`) / `xclip` — lectura de imágenes del portapapeles del sistema (dependencias `.deb`)

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
