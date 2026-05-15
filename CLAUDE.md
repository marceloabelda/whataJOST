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

- **Links externos (lib.rs:429, 537):** `window.open` override + `document.addEventListener('click', capture)` redirigen URLs no-WhatsApp a `window.location.href` → `on_navigation` las abre con `opener.open_url()` del plugin opener. Si `open_url()` falla, se lanza `xdg-open <url>` directamente como fallback (lib.rs:~372). Ambos intentos quedan en los logs.

- **Deep links `whatsapp://` (funciona ✓):**
  El sistema operativo puede abrir WhataJOST cuando el usuario hace clic en un link `whatsapp://send?phone=...` en el browser o en otra app.

  **Registro del protocolo:** `public/whatajost-url-handler.desktop` declara `MimeType=x-scheme-handler/whatsapp;` con `NoDisplay=true` (no aparece en lanzadores). El deb/rpm lo instala en `/usr/share/applications/` vía `bundle.linux.deb.files` en `tauri.conf.json`. El post-install corre `update-desktop-database` que lo activa como handler del sistema.

  **`handle_deep_link(app, url)` (Rust):** Convierte `whatsapp://path?query` → `https://web.whatsapp.com/path?query` (strip de `whatsapp://`, prepend de la base). Llama `show_window`, luego lanza un thread que espera 800 ms y evalúa `window.location.href = "<url>"` en la ventana `whatsapp-web`. El delay es necesario: si la app estaba cerrada, la ventana necesita tiempo para cargar `web.whatsapp.com` antes de poder navegar.

  **Dos rutas de entrada:**
  1. **App ya corriendo** — `tauri_plugin_single_instance` callback: el nuevo proceso le pasa `args` al proceso ya en marcha; si hay una URL `whatsapp://` en los args, se llama `handle_deep_link`.
  2. **Primera apertura** — `setup()`: lee `std::env::args()`, si encuentra `whatsapp://` lanza un thread que espera 5 s (para que la app cargue completamente) y llama `handle_deep_link`.
- **Menú contextual (lib.rs:616):** Handlers capture-phase en `contextmenu`/`mousedown`/`mouseup` bloquean los listeners de WhatsApp en campos editables (`isContentEditable`, `INPUT`, `TEXTAREA`) → aparece el menú nativo de WebKitGTK con corrección ortográfica.
- **Badge no leídos (lib.rs:856):** `MutationObserver` en `<title>` + `setInterval` cada 2,5 s. Parsea `(N) WhatsApp` del título y llama `update_unread_count` IPC.
- **Notificaciones (lib.rs:834):** Reemplaza `window.Notification` con `show_notification` IPC. `Notification.permission` devuelve `'granted'` permanentemente.
- **User agent (lib.rs:341):** La ventana `whatsapp-web` usa un UA de Chrome/Linux para que WhatsApp Web no detecte un navegador desconocido y bloquee el acceso.

---

#### Comportamientos Rust

**Cierre de ventana → hide (lib.rs:996):**
`on_window_event` con `CloseRequested` llama `api.prevent_close()` + `window.hide()`. La ventana de WhatsApp nunca se destruye; solo se oculta. Esto es intencional: destruirla y recrearla es lento y pierde la sesión si el proceso WebKit muere.

**`show_window` (lib.rs:96):** Llama `set_always_on_top(true)` → `show()` → `set_focus()` → `set_always_on_top(false)`. El truco `always_on_top` es necesario en algunos WMs (ej. i3, algunos GNOME) que ignoran `set_focus()` sin que la ventana sea temporalmente top-level.

> **⚠ No tocar:** el orden `always_on_top(true)` → `show` → `focus` → `always_on_top(false)`. Quitar cualquier paso puede hacer que la ventana no aparezca al frente en ciertos escritorios.

---

#### Bandeja del sistema (tray)

Ícono: `public/tray.png` (64×64), embebido en el binario con `include_bytes!` en tiempo de compilación.

**Comportamiento del click:**
- **Click izquierdo:** alterna show/hide de la ventana `whatsapp-web` (si estaba visible la oculta, si no la muestra). `show_menu_on_left_click(false)` — el menú solo aparece con click derecho.
- **Click derecho:** muestra el menú contextual.

**Menú del tray:**
| Ítem | Tipo | Comportamiento |
|---|---|---|
| Abrir WhatsApp | MenuItem | `show_window` |
| Iniciar con el sistema | CheckMenuItem | `tauri_plugin_autostart` enable/disable |
| Notificaciones emergentes | CheckMenuItem | toggle `NotificationPopupState`, persiste en `config.json` |
| Barra flotante | CheckMenuItem | toggle `FloatingBarState`, crea/cierra ventana `floating-bar`, persiste en `config.json` |
| Buscar actualización | MenuItem | `check_for_updates(app)` |
| [Submenu Uptime Kuma] | Submenu | visible si hay estado UK; `✓/✗ nombre` por monitor (no clickeables) |
| [Submenu Zabbix] | Submenu | visible si hay estado Zabbix; `✗ [Severidad] nombre` por problema (no clickeables) |
| Configurar Uptime Kuma | MenuItem | abre/enfoca ventana `uptime-kuma-config` |
| Configurar Zabbix | MenuItem | abre/enfoca ventana `zabbix-config` |
| Ver logs | MenuItem | abre/enfoca ventana `logs` |
| Salir | MenuItem | `app.exit(0)` |

El menú se reconstruye completamente en cada `update_tray_menu()` → `build_tray_menu()`. Los submenus UK y Zabbix son dinámicos (reflejan el estado en tiempo real) y solo aparecen si hay al menos un resultado conocido. `build_tray_menu` lee `UptimeKumaState` y `ZabbixState` directamente.

**Badge de no leídos + puntos de monitoreo (Rust):**
`regenerate_tray_icon` combina tres capas sobre el RGBA base (64×64 px):
1. **Fila inferior (UK):** punto verde (AllUp), puntos rojos ×N (Down(N)), punto naranja (Unreachable), nada (NotConfigured). Radio 5.5 px, separados 13 px, anclados a `y = 64 - 6.5`.
2. **Fila sobre UK (Zabbix):** misma lógica con `ZbxDotState`. `y = UK_Y - 13`.
3. **Badge superior derecho:** círculo rojo (radio 14, centro 48,16) con número pixel art (5×7, escala 2). Solo cuando `count > 0`. Muestra `9+` para counts > 9.

`draw_filled_circle` usa anti-aliasing manual via `blend_pixel`. Ningún crate de imágenes.

> **⚠ No tocar:** `TrayBadgeState.base_rgba` se captura una sola vez al inicio desde el PNG embebido; si se pierde, el badge no puede regenerarse. `FONT_SCALE = 2` calibra los dígitos para el ícono de 64×64.

---

#### Config persistente

`config.json` en `app.path().app_config_dir()` (Linux: `~/.config/whatajost/config.json`). Campos:
- `notifications_enabled` (bool, default `true`)
- `floating_bar_visible` (bool, default `false`)
- `uptime_kuma_url` (string, default vacío)
- `uptime_kuma_api_key` (string, default vacío)
- `zabbix_url` (string, default vacío)
- `zabbix_api_token` (string, default vacío)
- `zabbix_severities` (array de u8, default `[4, 5]` = Alto + Desastre)

Todas las escrituras usan `modify_config(app, |json| { ... })`: lee el JSON existente, aplica el closure, y escribe. Garantiza merge sin pisar otros campos y loggea errores de escritura. No hay migración de schema.

---

#### Uptime Kuma (funciona ✓)

Thread background (`do_uptime_kuma_poll`) corre en loop con `recv_timeout(30s)` — se puede triggear inmediatamente via `std::sync::mpsc::Sender<()>` (guardado en `UptimeKumaTrigger` state) cuando el usuario guarda la config.

**Flujo de polling:**
1. Lee `uptime_kuma_url` + `uptime_kuma_api_key` de config.json.
2. Si no hay config → limpia estado, ícono sin punto.
3. Si hay config → `GET <url>/metrics` con `Authorization: Basic <base64>` via `ureq`.
4. Si el path `/metrics` no está en la URL, se agrega automáticamente (sin duplicar).
5. Parsea formato Prometheus: líneas `monitor_status{monitor_name="...", ...} 1|0`.
6. Compara con estado previo (`UptimeKumaState`) → notifica si algún monitor cambia de UP↔DOWN.
7. Actualiza `TrayBadgeState.uk_dot` (AllUp/SomeDown/Unreachable) y llama `regenerate_tray_icon`.
8. Llama `update_tray_menu` para reconstruir el menú del tray.

**Autenticación Basic Auth (lib.rs:1246):** Uptime Kuma usa Basic Auth con usuario vacío y la API key como contraseña. Si el valor ya contiene `:` (formato `usuario:contraseña` o `:key`), se codifica tal cual en base64. Si no, se codifica `:api_key` (usuario vacío).

**Ícono del tray:** `regenerate_tray_icon` combina el punto UK (esquina inferior izquierda, radio 8) y el badge de mensajes (esquina superior derecha, radio 14). Los colores del punto: verde (AllUp), rojo (SomeDown), naranja (Unreachable), sin punto (NotConfigured).

**Menú del tray — Submenu UK:** `build_tray_menu` construye el menú completo en cada update. Si hay estado UK, agrega un `Submenu` con `✓/✗ NombreMonitor` (items no clickeables) y un header con el total. Ítem "Configurar Uptime Kuma" siempre visible.

**Ventana de config:** `public/uptime_kuma.html` — form con URL y API Key. Capability `uptime-kuma-window.json` para la ventana `uptime-kuma-config`. Comandos IPC: `get_uptime_kuma_config` (retorna `{url, api_key}`), `save_uptime_kuma_config` (guarda + trigerea poll inmediato). Tauri auto-renombra los args de snake_case a camelCase: el parámetro Rust `api_key` se invoca como `apiKey` desde JS.

> **⚠ No tocar:** el `recv_timeout` en el polling loop (es el mecanismo de trigger inmediato al guardar config). `regenerate_tray_icon` siempre lee `uk_dot` + `current_count` frescos del state — no cachear esos valores. La codificación Basic Auth con usuario vacío `:api_key`.

---

#### Zabbix (funciona ✓)

Thread background (`do_zabbix_poll`) idéntico al de UK: loop con `recv_timeout(30s)` + `ZabbixTrigger` para trigger inmediato.

**Flujo de polling:**
1. Lee `zabbix_url` + `zabbix_api_token` + `zabbix_severities` de config.json.
2. Si no hay config → limpia estado, ícono sin punto Zabbix.
3. Si hay config → `POST <url>/api_jsonrpc.php` con body JSON-RPC `problem.get` filtrado por severidades.
4. Si el path `/api_jsonrpc.php` no está en la URL, se agrega automáticamente.
5. Autenticación: solo header `Authorization: Bearer <token>`. No se envía `auth` en el body (Zabbix 6.0+ lo rechaza por no ser parte del estándar JSON-RPC 2.0).
6. Parsea array `result` → `Vec<ZabbixProblem>` (name + severity u8). `severity` se parsea como integer primero, string como fallback (Zabbix alterna el formato según versión).
7. Notifica problemas nuevos y resueltos (comparando con estado previo).
8. Actualiza `TrayBadgeState.zbx_dot` (Ok/Problems(n)/Unreachable) y reconstruye ícono y menú.

**Severidades:** `SEVERITY_LABELS = ["No clasificado", "Información", "Advertencia", "Promedio", "Alto", "Desastre"]` (índice 0–5). El usuario elige cuáles recibir; los valores se almacenan como `Vec<u8>`. El parámetro `severities` en el filtro `problem.get` se envía como array de enteros (ej. `[4, 5]`).

**Ventana de config:** `public/zabbix.html` — form con URL + API Token + chips de severidad (coloreados por nivel, default Alto+Desastre). Capability `zabbix-window.json` para la ventana `zabbix-config`. Comandos IPC: `get_zabbix_config` (retorna `{url, api_token, severities}`), `save_zabbix_config(url, apiToken, severities)` — notar que Tauri auto-renombra `api_token` → `apiToken` en los args. La respuesta de `get_zabbix_config` mantiene snake_case (`cfg.api_token`).

> **⚠ No tocar:** solo header `Authorization: Bearer`, sin `auth` en body. El parseo dual de `severity` (integer + string fallback) — Zabbix cambió el tipo entre versiones. Tauri auto-renombra args de comandos a camelCase (`api_token` → `apiToken`).

---

#### Barra flotante de monitoreo (funciona ✓)

Ventana `floating-bar` (420×44 px) siempre visible, sin decoraciones, transparente, `skip_taskbar`. Muestra estado de UK y Zabbix en tiempo real. Se activa/desactiva desde el tray → "Barra flotante".

**Comportamiento:**
- Al activar: `create_floating_bar_window` crea la ventana centrada horizontalmente, 10 px desde el tope del monitor primario.
- Al desactivar desde tray: `win.close()` + `FloatingBarState → false` + guarda config.
- Si el WM la cierra (Alt+F4, etc.): `on_window_event(CloseRequested)` sincroniza `FloatingBarState → false`, guarda config, reconstruye menú del tray.

**`get_monitoring_status` IPC:** devuelve `MonitoringBarStatus` con `uk_configured`, `uk_reachable`, `uk_monitors` (Vec de `{name, up}`), y los análogos de Zabbix. La barra llama este comando cada 5 s y renderiza el resultado.

**`public/floating_bar.html`:** pill semitransparente (glassmorphism). Sección UK | divider | Sección Zabbix. Puntos coloreados (verde=ok, rojo=caído/problema, amarillo=promedio Zabbix, gris=offline), texto de estado compacto, tooltip por punto con el nombre del monitor/problema. Toda la barra es `data-tauri-drag-region`.

> **⚠ No tocar:** el `on_window_event(CloseRequested)` en `create_floating_bar_window` — sin él, cerrar la ventana por el WM deja `FloatingBarState = true` (tray checkbox marcado) pero la ventana ya no existe.

---

#### Auto-updater (funciona ✓)

`check_for_updates_impl(app, silent)` — corre en background thread.
- **Al inicio:** llamado con `silent=true` después de 5 s (para no interferir con la carga inicial de WhatsApp Web).
- **Desde el menú:** llamado con `silent=false` (muestra diálogos de error si falla).

Flujo:
1. `tauri_plugin_updater` verifica `latest.json` en GitHub releases.
2. Si hay update: diálogo `OkCancel` con la versión nueva.
3. Descarga el paquete a un temp file (`.deb` en Linux, `.msi` en Windows).
4. **Linux — 4 intentos de instalación en cascada:**
   - `pkexec apt install --yes <file>` (Ubuntu moderno, maneja dependencias)
   - `pkexec dpkg -i <file>` (fallback sin resolver deps)
   - `zenity --password` + `sudo -S dpkg -i <file>` (fallback sin pkexec)
   - `xdg-open <file>` (abre el instalador gráfico del sistema; muestra aviso al usuario para que complete manualmente)
5. **Windows:** `msiexec /i <file> /quiet`
6. Si la instalación exitosa: relanza el binario nuevo con `sh -c "sleep 2 && exec '<exe>'"` (el delay de 2 s permite que el proceso viejo libere el lock de `tauri_plugin_single_instance`) → `app.exit(0)`.

> **⚠ No tocar:** el delay de 2 s antes de relanzar (sin él, el nuevo proceso ve el lock del viejo y no arranca). El orden de los 4 intentos Linux (el 4º abre un diálogo y retorna, no espera a que el usuario instale — si se invierte el orden, los intentos con pkexec no corren).

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
| `get_uptime_kuma_config` | Devuelve `{url, api_key}` de config.json |
| `save_uptime_kuma_config(url, api_key)` | Guarda config UK y trigerea poll inmediato |
| `get_zabbix_config` | Devuelve `{url, api_token, severities}` de config.json |
| `save_zabbix_config(url, api_token, severities)` | Guarda config Zabbix y trigerea poll inmediato |
| `get_monitoring_status` | Devuelve estado combinado UK + Zabbix (`MonitoringBarStatus`); usado por la barra flotante cada 5 s |

### Capabilities (IPC por ventana)

| Archivo | Ventana | Permisos |
|---|---|---|
| `default.json` | `main` (oculta, 1×1 px) | `core`, `opener`, `updater`, `dialog`, `autostart`, `allow-whatsapp-ipc` |
| `whatsapp-notification.json` | `whatsapp-web` (remota: `*.whatsapp.com`, `*.whatsapp.net`) | `core:default`, `clipboard-manager:allow-read-image`, `allow-whatsapp-ipc` |
| `logs-window.json` | `logs` | `core:default`, `allow-whatsapp-ipc` |
| `notification-window.json` | `notification` | `core:default`, `allow-whatsapp-ipc` |
| `uptime-kuma-window.json` | `uptime-kuma-config` | `core:default`, `allow-whatsapp-ipc` |
| `zabbix-window.json` | `zabbix-config` | `core:default`, `allow-whatsapp-ipc` |
| `floating-bar-window.json` | `floating-bar` | `core:default`, `allow-whatsapp-ipc` |

La ventana `whatsapp-web` carga una URL externa; el IPC solo funciona cuando la URL coincide con los patrones de `remote.urls`.

**Importante — ACL de Tauri 2.11.1+**: Desde Tauri 2.11.1, todos los comandos custom de la app (no solo plugins) requieren permiso ACL explícito cuando son invocados desde webviews remotos. El archivo `src-tauri/permissions/default.toml` define el permiso `allow-whatsapp-ipc` que lista todos los comandos IPC de la app. Sin ese archivo (o sin incluir `allow-whatsapp-ipc` en el capability de la ventana remota), los comandos fallan silenciosamente — no aparece ningún error en la consola JS ni en los logs de Rust.

### Ventanas HTML (`public/`)

- `logs.html` — Visor de logs en tiempo real (polling 2 s a `get_logs`); muestra error explícito si IPC no está disponible
- `notification.html` — Toast de notificación transparente (se auto-cierra a los 5,5 s)
- `uptime_kuma.html` — Configuración de Uptime Kuma (URL + API Key)
- `zabbix.html` — Configuración de Zabbix (URL + API Token + chips de severidad)
- `floating_bar.html` — Barra flotante de monitoreo (pill semitransparente, polling 5 s a `get_monitoring_status`)
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
