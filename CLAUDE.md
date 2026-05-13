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

El script se inyecta con `WebviewWindowBuilder::initialization_script` (lib.rs:384). Corre antes de que cargue cualquier JS de WhatsApp Web, dentro de un IIFE para no contaminar el scope global. Establece interceptores permanentes sobre los que WhatsApp Web opera sin saberlo.

---

#### Descargas (funciona ✓)

**Cómo genera WhatsApp Web una descarga:**
1. Crea el contenido como `Blob` y obtiene una `blob:` URL con `URL.createObjectURL(blob)`.
2. Crea `<a href="blob:..." download="nombre.ext">` y llama `.click()` programáticamente, o el usuario hace clic en un botón ya visible en el DOM.

**El problema:** WebKit2GTK no puede abrir el diálogo nativo de guardado desde JS. Hay que interceptar el blob antes de que el navegador intente descargarlo.

**`blobStore` (lib.rs:450):** Sobrescribe `URL.createObjectURL` para guardar cada `blob:URL → Blob` en un `Map`. Sobrescribe `URL.revokeObjectURL` para eliminar la entrada **60 segundos después** (no inmediatamente): si WhatsApp revoca la URL antes de que `FileReader` termine de leer, el blob sigue disponible. Sin este delay, la descarga falla en archivos grandes.

**Tres puntos de intercepción JS (el primero que aplica dispara `downloadBlob`):**

1. **`HTMLAnchorElement.prototype.click` (lib.rs:521):** Intercepta `.click()` programáticos. Si `this.href` empieza con `blob:`, llama `downloadBlob(url, this.download || 'archivo')` y retorna sin propagar. Cubre el camino más frecuente — WhatsApp Web crea el `<a>` y llama `.click()`.

2. **`document.addEventListener('click', capture, lib.rs:537):** Intercepta clics del usuario en `<a href="blob:...">` ya presentes en el DOM (botón de descarga visible). Llama `preventDefault` + `stopImmediatePropagation` y desvía a `downloadBlob`. También intercepta clics en links externos no-WhatsApp y los abre con `window.location.href`.

3. **`window.open` override (lib.rs:429):** Algunos flujos de exportación de WhatsApp usan `window.open(blobUrl)` en lugar de `<a>`. Se intercepta y redirige a `downloadBlob`.

**Cuarta intercepción Rust — `on_navigation` para `blob:` (lib.rs:327):** Si ninguno de los anteriores capturó el blob y WebKit intenta navegar a la `blob:` URL, Rust cancela la navegación (`return false`) y llama `window.__waDownloadBlob(url)` via `win.eval(...)`. `window.__waDownloadBlob` (lib.rs:518) es el alias global que expone `downloadBlob` para que Rust pueda llamarla.

**`downloadBlob(url, fileName)` (lib.rs:485):**
1. Busca el blob en `blobStore`. Si no está (revocado hace >60 s), hace `fetch(url)` para leerlo desde WebKit.
2. Llama `saveBlob(blob, fileName)`.

**`saveBlob(blob, fileName)` (lib.rs:464):**
1. Convierte el blob a base64 con `FileReader.readAsDataURL` (extrae la parte después de la coma).
2. Invoca IPC `save_file(data, fileName)` → Rust.

**`save_file` (Rust, lib.rs:1385):**
1. Decodifica base64 con `base64::engine::general_purpose::STANDARD`.
2. Abre `rfd::FileDialog::new().set_file_name(&file_name).save_file()`.
3. Si el diálogo no aparece (sin xdg-portal activo en la sesión) o el usuario cancela → guarda en `~/Downloads/WhataJOST/<fileName>` como fallback y lo registra en logs.

> **⚠ No tocar:** el delay de 60 s en `blobStore` (necesario para archivos grandes), el override de `prototype.click` (es el camino principal de WhatsApp), el `on_navigation` para `blob:` (es el safety net Rust), y el orden de los tres interceptores JS.

---

#### Paste de imágenes (funciona ✓)

Handler capture-phase en `document` (lib.rs:601). Corre antes que cualquier handler de WhatsApp. Las cuatro ramas se evalúan en orden estricto — no reordenar.

**Rama 1 — ya hay archivos (lib.rs:603):** `clipboardData.files.length > 0` → el paste ya trae archivos (archivo real del SO, o paste sintético que generamos nosotros en ramas 2 y 4). Se deja pasar a WhatsApp sin modificar. Esta rama actúa como punto de llegada para los pastes sintéticos que generamos en las otras ramas.

**Rama 2 — imagen en `clipboardData.items` (lib.rs:612):** WebKit2GTK expone la imagen directamente cuando fue copiada dentro del mismo WebView (e.g., captura en WhatsApp Web). Se llama `imageItem.getAsFile()`, se arma un `DataTransfer` con ese `File` y se despacha un `ClipboardEvent` sintético al elemento activo. Ese evento re-entra al handler pero cae en Rama 1 y llega a WhatsApp.

**Rama 3 — texto plano (lib.rs:644):** `clipboardData.types` contiene `text/plain` o `text/rtf` → es texto real. Se deja pasar a WhatsApp sin tocar.

> **⚠ Por qué `text/html` NO está en la Rama 3:** Firefox/Chrome al copiar una imagen ponen `image/png` en el clipboard del sistema **más** un `<img>` en `text/html` como fallback. WebKit2GTK **no expone** el `image/png` externo en `clipboardData.items`, solo el `text/html`. Si ese HTML se pasara a WhatsApp, lo recibiría como texto HTML, no como imagen. Por eso `text/html` solo → cae a Rama 4 y se trata como posible imagen vía `wl-paste`.

**Rama 4 — sin texto / sin archivos / sin imagen (lib.rs:652):** Cubre `text/html` solo, clipboard vacío, imagen de app externa (Firefox, GIMP, etc.). Pasos:
1. Detener el evento **sincrónicamente** con `preventDefault` + `stopImmediatePropagation` **antes de cualquier `await`**. Si se esperara, WhatsApp procesaría el paste vacío mientras llega la respuesta IPC.
2. Invocar `read_clipboard_image` IPC → Rust llama `wl-paste --type image/png|jpeg|bmp|webp` (o `xclip`). La herramienta disponible se detecta la primera vez y se guarda en `OnceLock` para no repetir la detección.
3. Si devuelve `null` → log de warning, no se pega nada.
4. Si devuelve `{data: base64, mime_type}` → decodifica a `Uint8Array`, crea `Blob` + `File('clipboard.ext')`, arma `DataTransfer`, despacha `ClipboardEvent` sintético al elemento activo → cae en Rama 1 → WhatsApp lo recibe como imagen.

> **⚠ No tocar:** el orden de las ramas (especialmente Rama 2 antes de Rama 3), la exclusión de `text/html` de la Rama 3, y el `stopImmediatePropagation` sincrónico antes del `await` en Rama 4.

---

#### Drag & drop de archivos (funciona ✓)

**El problema fundamental en Linux/Wayland:** WebKit2GTK consume los eventos DnD a nivel GTK antes de que lleguen al DOM o al sistema de Tauri. Un archivo arrastrado desde Nautilus no dispara `dataTransfer.files`, ni `text/uri-list` en el DOM, ni `DragDropEvent` de Tauri. Solo la señal GTK nativa lo intercepta.

**Cuatro capas. La Capa 1 es la que realmente funciona en Wayland; las demás son fallbacks para otros entornos.**

**Capa 1 — GTK `drag-data-received` (Rust, lib.rs:887) — capa primaria:**
Usando `window.with_webview`, se llama `widget.drag_dest_add_uri_targets()` (trait `gtk::prelude::WidgetExt`) para registrar `text/uri-list` como tipo aceptado (aditivo, no reemplaza la configuración interna de WebKit). Luego se conecta `widget.connect_drag_data_received(...)`. El handler:
1. Extrae URIs de `gtk::SelectionData` → `Vec<glib::GString>`.
2. Lanza un `std::thread::spawn` (para no bloquear el loop GTK; sin esto la UI se congela).
3. Por cada URI `file://`: llama `decode_file_uri(uri)` (lib.rs:1024, percent-decoding UTF-8), lee el archivo con `std::fs::read`, detecta MIME con `mime_guess`, codifica en base64.
4. Llama `window.__tauriInjectDrop(name, b64, mime)` vía `win.eval()`.

**Capa 2 — DOM `drop` event (lib.rs:721, capture phase):**
Fallback para X11 o WebKit que sí expone archivos al DOM. Si `dataTransfer.files.length > 0` → arma `DataTransfer` y llama `dispatchPasteWithFiles(dt)`. Si `files` está vacío → lee `text/uri-list`: para `file://` invoca `read_file_for_drop` IPC (lib.rs:1233); para `https://` hace `fetch`. Luego llama `dispatchPasteWithFiles(dt)`.

**Capa 3 — Tauri `DragDropEvent` (Rust, lib.rs:962, `on_webview_event`):**
Fallback para plataformas donde Tauri intercepta el drop antes del WebView. Lee los paths, detecta MIME con `mime_guess`, codifica en base64, y llama `window.dispatchPasteWithFiles(dt)` via `eval()`.

**Capa 4 — `on_navigation file://` (Rust, lib.rs:341):**
Si WebKit intenta navegar a `file://` (comportamiento de X11 antiguo): Rust cancela la navegación, lee el archivo, llama `window.__tauriInjectDrop`. Log con prefijo `on_navigation file:// (fallback):` para distinguirlo de Capa 1 en los logs.

**Funciones JS que unen todo:**

**`window.__tauriInjectDrop(fileName, base64Data, mimeType)` (lib.rs:847):**
Convierte base64 → `Uint8Array` → `Blob` → `File`, lo agrega a un `DataTransfer` nuevo y llama `dispatchPasteWithFiles(dt)`. Es el puente entre el código Rust (que leyó el archivo del sistema) y el DOM de WhatsApp.

**`window.dispatchPasteWithFiles(dt)` (lib.rs:409):**
Despacha un `ClipboardEvent('paste')` sintético. Usa `window.__waLastFocusedEditable` (lib.rs:402) como target — se rastrea el último elemento editable que recibió foco en capture phase, porque durante el drag el foco puede haberse ido a `document.body`. Si el target no es `document.body`, llama `.focus()` antes de despachar. WhatsApp Web acepta `ClipboardEvent` sintéticos con `clipboardData.files`, los procesa como un paste real.

> **⚠ Por qué se usa `paste` en lugar de `drop`:** WhatsApp Web tiene `isTrusted` checks en sus handlers de drag que bloquean eventos sintéticos. Los `ClipboardEvent` sintéticos no tienen ese check.

> **⚠ No tocar:** `widget.drag_dest_add_uri_targets()` (sin esto el widget puede no aceptar URIs), el `std::thread::spawn` en Capa 1 (sin él el loop GTK se bloquea), `window.__waLastFocusedEditable` (sin esto el paste llega a `document.body` y WhatsApp lo ignora).

---

- **Links externos (lib.rs:429, 537):** `window.open` override + `document.addEventListener('click', capture)` redirigen URLs no-WhatsApp a `window.location.href` → `on_navigation` las abre con `opener.open_url()` del sistema.
- **Menú contextual (lib.rs:584):** Handlers capture-phase en `contextmenu`/`mousedown`/`mouseup` bloquean los listeners de WhatsApp en campos editables (`isContentEditable`, `INPUT`, `TEXTAREA`) → aparece el menú nativo de WebKitGTK con corrección ortográfica.
- **Badge no leídos (lib.rs:812):** `MutationObserver` en `<title>` + `setInterval` cada 2,5 s. Parsea `(N) WhatsApp` del título y llama `update_unread_count` IPC.
- **Notificaciones (lib.rs:791):** Reemplaza `window.Notification` con `show_notification` IPC. `Notification.permission` devuelve `'granted'` permanentemente.

---

### Comandos Tauri (IPC)

| Comando | Descripción |
|---|---|
| `save_file(data, file_name)` | Abre `rfd::FileDialog`; si el diálogo no aparece (xdg-portal), guarda en `~/Downloads/WhataJOST/` y lo registra en logs |
| `read_clipboard_image` | Lee imagen del portapapeles del sistema con `wl-paste` o `xclip` (detecta herramienta disponible con `OnceLock`) |
| `read_file_for_drop(path)` | Lee un archivo local por path (usado desde el DOM drop handler cuando `dataTransfer.files` está vacío en Linux) |
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

**Importante — ACL de Tauri 2.11.1+**: Desde Tauri 2.11.1, todos los comandos custom de la app (no solo plugins) requieren permiso ACL explícito cuando son invocados desde webviews remotos. El archivo `src-tauri/permissions/default.toml` define el permiso `allow-whatsapp-ipc` que lista los 11 comandos IPC de la app. Sin ese archivo (o sin incluir `allow-whatsapp-ipc` en el capability de la ventana remota), los comandos fallan silenciosamente — no aparece ningún error en la consola JS ni en los logs de Rust.

### Ventanas HTML (`public/`)

- `logs.html` — Visor de logs en tiempo real (polling 2 s a `get_logs`); muestra error explícito si IPC no está disponible
- `notification.html` — Toast de notificación transparente (se auto-cierra)
- `index.html` — Página mínima para la ventana `main` (oculta)

## Key dependencies

- `rfd` v0.17 con features `xdg-portal` + `wayland` — diálogos de archivo nativos
- `tauri-plugin-dialog` con feature `xdg-portal` — diálogos de mensaje nativos (updater, errores)
- `webkit2gtk` v2 (Linux) — corrección ortográfica + devtools en producción
- `glib` v0.18 + `gtk` v0.18 (Linux) — señales GTK para interceptar drag & drop a nivel nativo (ya eran transitivas vía webkit2gtk; se agregan como deps directas para usar `WidgetExt::connect_drag_data_received`)
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
