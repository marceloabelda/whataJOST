# WhataJOST

WhatsApp Web wrapper built with Tauri v2 (Rust backend + WebKitGTK webview).

## Workflow

Para cada tarea, seguí estos pasos en orden:

1. **Buscar** — Usá `mcp__codebase-search__search_codebase` (si está disponible), más la documentación del proyecto y los archivos fuente relevantes para entender el contexto antes de escribir código.
2. **Codificar** — Implementá los cambios necesarios.
3. **Documentar** — Mantené actualizado este CLAUDE.md si cambiaron decisiones de arquitectura, flujos, o dependencias.
4. **Commit y push** — Hacé commit con mensaje descriptivo en español y pusheá con `git push origin main` (empuja a GitHub y git.jost.ar en simultáneo). El pull solo se hace desde GitHub.

## Architecture

- `src-tauri/src/lib.rs` — Solo `run()`: registro de plugins y comandos (`generate_handler!`), `setup()` (estados, ventanas, tray, updater, atajo global) y eventos de ventana. Declara los módulos y los re-importa con `use modulo::*;`
- Módulos (cada uno arranca con `use super::*;` para ver los imports y los ítems de los demás; lo que se comparte entre módulos es `pub(crate)`):
  - `window.rs` — creación de las ventanas WhatsApp (`create_whatsapp_window_impl`: `on_navigation`, `on_page_load`, drag & drop GTK, ajustes WebKit), `show_window*`, deep links, `target_monitor`, comando `open_whatsapp`
  - `tray.rs` — menú del tray (`build_tray_menu`), badge pixel art (`regenerate_tray_icon`), comando `update_unread_count`
  - `notifications.rs` — notificaciones nativas (Linux), toast HTML (otras plataformas), comandos `show_notification`/`close_notification`
  - `downloads.rs` — descargas en partes (`save_file_*`), `unique_path`, `whatajost_downloads_dir`
  - `clipboard.rs` — lectura de imágenes del portapapeles (`wl-paste`/`xclip`) y de archivos para drag & drop
  - `config.rs` — `config.json` (`modify_config`, `load_*`/`save_*`) y estados persistidos
  - `updater.rs` — `check_for_updates_impl` (cascada de instalación .deb, AppImage, Windows)
  - `logging.rs` — `log_message`, buffer de logs, comandos de logs y ventana `logs`
  - `i18n.rs` — textos ES/EN (`t()`)
  - `resources.rs` — comando `get_resource_usage` (CPU y memoria de la app y sus procesos WebKit, solo Linux) para la pestaña "Recursos" de la ventana de logs
- Al agregar un comando IPC: `#[tauri::command] pub(crate) fn` en su módulo, registrarlo en `generate_handler!` con path completo (`modulo::comando`) y sumarlo a `permissions/default.toml`
- `src-tauri/src/inject.js` — Script inyectado en las ventanas de WhatsApp Web (embebido con `include_str!`)
- `src-tauri/src/constants.rs` — Constantes de configuración (delays, intervalos, geometría del badge)
- `.github/workflows/check.yml` — Chequeos (sintaxis de `inject.js` + clippy); `release.yml` — build y publicación
- `src-tauri/Cargo.toml` — Dependencias Rust
- `src-tauri/capabilities/` — Permisos Tauri por ventana
- `src-tauri/tauri.conf.json` — Configuración de Tauri
- `latest.json` — Metadata del auto-updater

### Script inyectado (`src-tauri/src/inject.js`)

El script vive en `src-tauri/src/inject.js` y se embebe en el binario con `include_str!`; se inyecta con `WebviewWindowBuilder::initialization_script` en `create_whatsapp_window_impl`. Validar la sintaxis con `node --check src-tauri/src/inject.js` (lo hacen CI y `release.sh`): un error de sintaxis compila igual y recién falla dentro de WhatsApp Web. Corre antes de que cargue cualquier JS de WhatsApp Web, dentro de un IIFE para no contaminar el scope global. Establece interceptores permanentes sobre los que WhatsApp Web opera sin saberlo.

> Este documento no usa números de línea (se desactualizan con cada cambio): para ubicar algo, buscar por nombre de función o variable con `grep -n`.

---

#### Descargas (funciona ✓)

**Cómo genera WhatsApp Web una descarga:**
1. Crea el contenido como `Blob` y obtiene una `blob:` URL con `URL.createObjectURL(blob)`.
2. Crea `<a href="blob:..." download="nombre.ext">` y llama `.click()` programáticamente, o el usuario hace clic en un botón ya visible en el DOM.

**El problema:** WebKit2GTK no puede abrir el diálogo nativo de guardado desde JS. Hay que interceptar el blob antes de que el navegador intente descargarlo.

**`blobStore`:** Sobrescribe `URL.createObjectURL` para guardar cada `blob:URL → Blob` en un `Map`. Sobrescribe `URL.revokeObjectURL` para eliminar la entrada **60 segundos después** (no inmediatamente): si WhatsApp revoca la URL antes de que `FileReader` termine de leer, el blob sigue disponible. Sin este delay, la descarga falla en archivos grandes.

**Tres puntos de intercepción JS (el primero que aplica dispara `downloadBlob`):**

1. **`HTMLAnchorElement.prototype.click`:** Intercepta `.click()` programáticos. Si `this.href` empieza con `blob:`, llama `downloadBlob(url, this.download || 'archivo')` y retorna sin propagar. Cubre el camino más frecuente — WhatsApp Web crea el `<a>` y llama `.click()`.

2. **`document.addEventListener('click', …, true)` (capture phase):** Intercepta clics del usuario en `<a href="blob:...">` ya presentes en el DOM (botón de descarga visible). Llama `preventDefault` + `stopImmediatePropagation` y desvía a `downloadBlob`. También intercepta clics en links externos no-WhatsApp y los abre con `window.location.href`.

3. **`window.open` override:** Algunos flujos de exportación de WhatsApp usan `window.open(blobUrl)` en lugar de `<a>`. Se intercepta y redirige a `downloadBlob`.

**Cuarta intercepción Rust — `on_navigation` para `blob:` (`on_navigation` en `create_whatsapp_window_impl`):** Si ninguno de los anteriores capturó el blob y WebKit intenta navegar a la `blob:` URL, Rust cancela la navegación (`return false`) y llama `window.__waDownloadBlob(url)` via `win.eval(...)`. `window.__waDownloadBlob` es el alias global que expone `downloadBlob` para que Rust pueda llamarla.

**`downloadBlob(url, fileName)`:**
1. Busca el blob en `blobStore`. Si no está (revocado hace >60 s), hace `fetch(url)` para leerlo desde WebKit.
2. Llama `saveBlob(blob, fileName)`.

**`saveBlob(blob, fileName)` — envío en partes:**
1. `save_file_begin` (IPC) → Rust crea un archivo temporal vacío en `<app_cache_dir>/descargas/<id>.part` y devuelve el `id`. En la primera descarga de la sesión (`id == 0`) borra restos `.part` de sesiones anteriores.
2. Recorre el blob en trozos de 4 MB (`blob.slice`), cada uno a base64 con `FileReader.readAsDataURL`, y llama `save_file_chunk(id, data)`. Es `async fn` en Rust: decodifica y agrega al temporal **fuera del hilo principal**.
3. Si algo falla, `save_file_abort(id)` borra el temporal.
4. `save_file(id, fileName)` → Rust.

> **⚠ Por qué en partes y no IPC binario:** la CSP de WhatsApp (`connect-src`) no permite `ipc://`, así que en la ventana `whatsapp-web` el IPC de Tauri siempre cae a `postMessage` con JSON. Ahí un `Uint8Array` se serializa como array de números (~3,5× el tamaño), peor que base64 (1,33×). Antes se mandaba el archivo entero en un solo base64: un video de 100 MB generaba varias copias completas simultáneas (data URL, split, JSON, `String` y `Vec` en Rust) y la decodificación corría en el hilo principal. En partes, el pico extra es ~un trozo.

**`save_file` (Rust):**
1. Abre `rfd::FileDialog::new().set_file_name(&file_name).save_file()`.
2. Mueve el temporal al destino con `move_upload` (`rename`; si falla por estar en otro filesystem, copia y borra).
3. Si el diálogo no aparece (sin xdg-portal activo en la sesión) o el usuario cancela → lo mueve a `~/Downloads/WhataJOST/<fileName>` como fallback y lo registra en logs.
4. Siempre borra el temporal al final (si ya se movió no existe; si hubo error, no queda basura).

> **⚠ No tocar:** el delay de 60 s en `blobStore` (necesario para archivos grandes), el override de `prototype.click` (es el camino principal de WhatsApp), el `on_navigation` para `blob:` (es el safety net Rust), y el orden de los tres interceptores JS. Al agregar comandos IPC nuevos, sumarlos a `permissions/default.toml` (ver "ACL de Tauri 2.11.1+").

---

#### PDFs — descarga con visor externo opcional (funciona ✓)

**El problema original:** clickear "descargar" dentro del visor de PDF de WhatsApp Web podía colgar la app intermitentemente. La hipótesis inicial (visor nativo embebido de WebKit2GTK interceptado a nivel `decide-policy`/`download-started`) resultó **incorrecta** tras inspeccionar los logs reales: WhatsApp Web no le pasa el PDF a WebKit como una respuesta HTTP `application/pdf` — abre su propio visor, una app HTML/JS servida en un iframe sandbox (`https://webtp.whatsapp.net/pdf-viewer`, `mime=text/html`). El botón "Descargar" de ese visor **no vive dentro del iframe**: es parte del DOM del frame top (`web.whatsapp.com`), y su click ya caía en el mecanismo de blobs existente (sección "Descargas" más arriba: `HTMLAnchorElement.prototype.click` override → `downloadBlob` → `saveBlob` → IPC `save_file`). El cuelgue original no tenía relación con el visor nativo de WebKit; el fix real fue mucho más simple de lo que se pensó al principio.

**Toggle en el tray:** "Descargar PDF abre con visor externo" (`CheckMenuItem` id `toggle_pdf_reader`, `build_tray_menu`). Estado en `PdfExternalReaderState` (`Mutex<bool>`), persistido en `config.json` como `pdf_external_reader` (default `false`) vía `load_pdf_external_reader`/`save_pdf_external_reader`.

**Mecanismo (todo dentro de `save_file`,):** el flujo de blobs ya existente termina siempre invocando la IPC `save_file(id, file_name)`, sin importar si el blob viene de un documento, una imagen o cualquier otro archivo. Al principio de `save_file`, si `file_name` termina en `.pdf` (case-insensitive) **y** el toggle está activo:
1. Mueve el temporal directo a `~/Downloads/WhataJOST/<nombre>` (vía `whatajost_downloads_dir`, helper compartido con el fallback normal de `save_file`), evitando pisar archivos existentes con `unique_path` (agrega `(1)`, `(2)`, etc.).
2. Abre el archivo con `opener().open_path(...)` (con fallback a `xdg-open` en Linux si falla, mismo patrón que "Links externos").
3. Devuelve el path sin pasar por `rfd::FileDialog` — no aparece ningún diálogo.

Si el toggle está desactivado, o el archivo no es un PDF, `save_file` sigue su comportamiento normal (diálogo de guardado / fallback a `~/Downloads/WhataJOST` si no hay diálogo disponible).

**La vista previa embebida del PDF (el iframe `pdf-viewer`) no se ve afectada** — sigue mostrándose igual que antes. El toggle solo cambia qué pasa al clickear "Descargar" dentro de ese visor. Se evaluó auto-clickear el botón de descarga (identificado como `[aria-label="Descargar"]`, con ícono `data-icon="ic-download"`) para saltear el preview por completo, pero se descartó: dependería de texto/atributos internos de la UI de WhatsApp que pueden cambiar sin aviso.

---

#### Paste de imágenes (funciona ✓)

Handler capture-phase en `document`. Corre antes que cualquier handler de WhatsApp. Las cuatro ramas se evalúan en orden estricto — no reordenar.

**Rama 1 — ya hay archivos:** `clipboardData.files.length > 0` → el paste ya trae archivos (archivo real del SO, o paste sintético que generamos nosotros en ramas 2 y 4). Se deja pasar a WhatsApp sin modificar. Esta rama actúa como punto de llegada para los pastes sintéticos que generamos en las otras ramas.

**Rama 2 — imagen en `clipboardData.items`:** WebKit2GTK expone la imagen directamente cuando fue copiada dentro del mismo WebView (e.g., captura en WhatsApp Web). Se llama `imageItem.getAsFile()`, se arma un `DataTransfer` con ese `File` y se despacha un `ClipboardEvent` sintético al elemento activo. Ese evento re-entra al handler pero cae en Rama 1 y llega a WhatsApp.

**Rama 3 — texto plano:** `clipboardData.types` contiene `text/plain` o `text/rtf` → es texto real. Se deja pasar a WhatsApp sin tocar.

> **⚠ Por qué `text/html` NO está en la Rama 3:** Firefox/Chrome al copiar una imagen ponen `image/png` en el clipboard del sistema **más** un `<img>` en `text/html` como fallback. WebKit2GTK **no expone** el `image/png` externo en `clipboardData.items`, solo el `text/html`. Si ese HTML se pasara a WhatsApp, lo recibiría como texto HTML, no como imagen. Por eso `text/html` solo → cae a Rama 4 y se trata como posible imagen vía `wl-paste`.

**Rama 4 — sin texto / sin archivos / sin imagen:** Cubre `text/html` solo, clipboard vacío, imagen de app externa (Firefox, GIMP, etc.). Pasos:
1. Detener el evento **sincrónicamente** con `preventDefault` + `stopImmediatePropagation` **antes de cualquier `await`**. Si se esperara, WhatsApp procesaría el paste vacío mientras llega la respuesta IPC.
2. Invocar `read_clipboard_image` IPC → Rust llama `wl-paste --type image/png|jpeg|bmp|webp` (o `xclip`). La herramienta disponible se detecta la primera vez y se guarda en `OnceLock` para no repetir la detección.
3. Si devuelve `null` → log de warning, no se pega nada.
4. Si devuelve `{data: base64, mime_type}` → decodifica a `Uint8Array`, crea `Blob` + `File('clipboard.ext')`, arma `DataTransfer`, despacha `ClipboardEvent` sintético al elemento activo → cae en Rama 1 → WhatsApp lo recibe como imagen.

> **⚠ No tocar:** el orden de las ramas (especialmente Rama 2 antes de Rama 3), la exclusión de `text/html` de la Rama 3, y el `stopImmediatePropagation` sincrónico antes del `await` en Rama 4.

---

#### Drag & drop de archivos (funciona ✓)

**El problema fundamental en Linux/Wayland:** WebKit2GTK consume los eventos DnD a nivel GTK antes de que lleguen al DOM o al sistema de Tauri. Un archivo arrastrado desde Nautilus no dispara `dataTransfer.files`, ni `text/uri-list` en el DOM, ni `DragDropEvent` de Tauri. Solo la señal GTK nativa lo intercepta.

**Cuatro capas. La Capa 1 es la que realmente funciona en Wayland; las demás son fallbacks para otros entornos.**

**Capa 1 — GTK `drag-data-received` (Rust, `with_webview` en `create_whatsapp_window_impl`) — capa primaria:**
Usando `window.with_webview`, se llama `widget.drag_dest_add_uri_targets()` (trait `gtk::prelude::WidgetExt`) para registrar `text/uri-list` como tipo aceptado (aditivo, no reemplaza la configuración interna de WebKit). Luego se conecta `widget.connect_drag_data_received(...)`. El handler:
1. Extrae URIs de `gtk::SelectionData` → `Vec<glib::GString>`.
2. Lanza un `std::thread::spawn` (para no bloquear el loop GTK; sin esto la UI se congela).
3. Por cada URI `file://`: llama `decode_file_uri(uri)` (percent-decoding UTF-8) y registra la ruta en `NATIVE_DROPS`.
4. Llama `inject_files_into_page` (ver "Funciones que unen todo").

**Capa 2 — DOM `drop` event (capture phase):**
Fallback para X11 o WebKit que sí expone archivos al DOM. Si `dataTransfer.files.length > 0` → arma `DataTransfer` y llama `dispatchPasteWithFiles(dt)`. Si `files` está vacío → lee `text/uri-list`: para `file://` invoca `read_file_for_drop` IPC; para `https://` hace `fetch`. Luego llama `dispatchPasteWithFiles(dt)`.

**Capa 3 — Tauri `DragDropEvent` (Rust, `on_webview_event`):**
Fallback para plataformas donde Tauri intercepta el drop antes del WebView. Registra los paths (en `Enter` y `Drop`) y llama `inject_files_into_page` desde un thread aparte.

**Capa 4 — `on_navigation file://` (Rust, `on_navigation`):**
Si WebKit intenta navegar a `file://` (comportamiento de X11 antiguo): Rust cancela la navegación y, solo si la ruta vino de un arrastre nativo reciente (`is_native_drop`), llama `inject_files_into_page`. Log con prefijo `on_navigation file:// (fallback):` para distinguirlo de Capa 1 en los logs.

**Funciones que unen todo:**

**`inject_files_into_page(app, win, paths)` (Rust, `clipboard.rs`) + `window.__waDrop*` (JS):**
Es el puente entre el código Rust (que lee el archivo del sistema) y el DOM de WhatsApp, usado por las capas 1, 3 y 4. Por cada archivo: `__waDropBegin(id, name, mime)`, después lo lee de a `DROP_CHUNK_BYTES` (3 MB) y manda cada trozo con `__waDropChunk(id, b64)` en un `eval` propio; JS decodifica cada trozo a `Uint8Array` al llegar. Al final `__waDropDispatch([ids])` arma un `File` por id (a partir de las partes binarias), los junta en un único `DataTransfer` y llama `dispatchPasteWithFiles(dt)`; si un archivo falla, `__waDropAbort(id)`. Antes era un solo `eval` con el archivo entero en base64: WebKit tenía que parsear un script ~1,33× el tamaño del archivo y un video grande trababa la UI. Llamar siempre desde un thread aparte (lee disco).

**`window.dispatchPasteWithFiles(dt)`:**
Despacha un `ClipboardEvent('paste')` sintético. Usa `window.__waLastFocusedEditable` como target — se rastrea el último elemento editable que recibió foco en capture phase, porque durante el drag el foco puede haberse ido a `document.body`. Si el target no es `document.body`, llama `.focus()` antes de despachar. WhatsApp Web acepta `ClipboardEvent` sintéticos con `clipboardData.files`, los procesa como un paste real.

> **⚠ Por qué se usa `paste` en lugar de `drop`:** WhatsApp Web tiene `isTrusted` checks en sus handlers de drag que bloquean eventos sintéticos. Los `ClipboardEvent` sintéticos no tienen ese check.

> **⚠ No tocar:** `widget.drag_dest_add_uri_targets()` (sin esto el widget puede no aceptar URIs), el `std::thread::spawn` en Capa 1 (sin él el loop GTK se bloquea), `window.__waLastFocusedEditable` (sin esto el paste llega a `document.body` y WhatsApp lo ignora).

---

- **Seguridad — la página es contenido remoto, no confiar en lo que manda:**
  - **Nombres de archivo** (`sanitize_file_name` en `downloads.rs`, con tests): el `file_name` de `save_file` viene del atributo `download` (normalmente el nombre que puso quien mandó el archivo) y se usa con `dir.join(...)` en el fallback y en el modo PDF externo. Se reduce al nombre base y se reemplazan caracteres inválidos en Windows; sin esto, `../../.config/autostart/x.desktop` o una ruta absoluta escribían fuera de la carpeta de descargas (bastaba con cancelar el diálogo de guardar).
  - **Lectura de archivos locales** (`NATIVE_DROPS` en `clipboard.rs`): `read_file_for_drop` (IPC) y la navegación a `file://` (Capa 4 de drag & drop) los puede disparar el JS de la página por su cuenta, así que solo leen rutas registradas por un arrastre nativo (señal GTK de la Capa 1 o `DragDropEvent` Enter/Drop de la Capa 3) en los últimos `NATIVE_DROP_ALLOW_SECS` (30 s). Sin esto, cualquier script en web.whatsapp.com podía leer `~/.ssh/id_rsa`.
  - **Dominios** (`on_navigation`): se navega dentro de la app solo con `http(s)` y dominio exacto o subdominio de `whatsapp.com`, `whatsapp.net`, `facebook.com`, `fbcdn.net` (`host == d || host.ends_with(".d")`). Antes era `ends_with("whatsapp.com")`, que aceptaba `evilwhatsapp.com`.
  - **Esquemas externos**: a `opener`/`xdg-open` solo van `http`, `https`, `mailto` y `tel`; el resto (`smb:`, `sftp:`, handlers de otras apps) se bloquea y se loggea.
- **Links externos:** `window.open` override + `document.addEventListener('click', capture)` en `inject.js`. Para links `http(s)`/`mailto`/`tel` cuyo host **no** es `web.whatsapp.com` (`isExternalLink`), cancelan el click y llaman al comando IPC `open_external(url)`, que valida el esquema y abre con `opener.open_url()` (fallback `xdg-open`). Antes usaban `window.location.href` y una lista JS que trataba `facebook.com` (y todo `*.whatsapp.com`) como interna: esos links no se interceptaban, WhatsApp los abría con `window.open` y el override los descartaba en silencio (links de Facebook que "no abrían"). La lista de dominios de `on_navigation` (Rust) es otra cosa y sigue incluyendo dominios de Meta: decide qué puede cargarse en el WebView, incluidos iframes de WhatsApp (pagos en `*.facebook.com`, visor de PDF en `webtp.whatsapp.net`); si igual llega ahí una navegación externa, la abre en el navegador.

- **Deep links `whatsapp://` (funciona ✓):**
  El sistema operativo puede abrir WhataJOST cuando el usuario hace clic en un link `whatsapp://send?phone=...` en el browser o en otra app.

  **Registro del protocolo:** `public/whatajost-url-handler.desktop` declara `MimeType=x-scheme-handler/whatsapp;` con `NoDisplay=true` (no aparece en lanzadores). El deb lo instala en `/usr/share/applications/` vía `bundle.linux.deb.files` en `tauri.conf.json`. El post-install corre `update-desktop-database` que lo activa como handler del sistema.

  **`handle_deep_link(app, url)` (Rust):** Convierte `whatsapp://path?query` → `https://web.whatsapp.com/path?query` (strip de `whatsapp://`, prepend de la base) y llama `show_window`. La navegación (`navigate_whatsapp`, vía `WebviewWindow::navigate`) **no usa esperas fijas**: `DEEP_LINK_STATE` (`Mutex<(cargada, pendiente)>`) guarda si la ventana `whatsapp-web` ya terminó de cargar. Si ya cargó, navega en el acto; si no, deja la URL pendiente y la navega `on_whatsapp_loaded`, que se llama desde `on_page_load` (`PageLoadEvent::Finished`) de esa ventana. Las dos cosas van bajo el mismo lock para que no haya carrera entre "terminó de cargar" y "llegó el link". Antes había un `sleep` de 5 s al arrancar más 800 ms antes de navegar: en máquinas lentas el link se perdía y en rápidas sobraba.

  **Dos rutas de entrada:**
  1. **App ya corriendo** — `tauri_plugin_single_instance` callback: el nuevo proceso le pasa `args` al proceso ya en marcha; si hay una URL `whatsapp://` en los args, se llama `handle_deep_link`.
  2. **Primera apertura** — `setup()`: lee `std::env::args()` y, si encuentra `whatsapp://`, llama `handle_deep_link` directamente (la ventana ya fue creada; el link queda pendiente hasta que cargue).
- **Menú contextual:** Handlers capture-phase en `contextmenu`/`mousedown`/`mouseup` bloquean los listeners de WhatsApp en campos editables (`isContentEditable`, `INPUT`, `TEXTAREA`) → aparece el menú nativo de WebKitGTK con corrección ortográfica.
- **Badge no leídos:** `MutationObserver` en `<title>` parsea `(N) WhatsApp` y llama `update_unread_count` IPC (solo si el número cambió). Hasta que aparece el `<title>`, un observer temporal mira el DOM completo y **se desconecta apenas lo encuentra** — dejarlo conectado lo dispararía con cada mutación de WhatsApp durante toda la sesión. Un `setInterval` de 30 s funciona de red de seguridad: si WhatsApp reemplazó el elemento `<title>`, re-engancha el observer.
- **Notificaciones:** Reemplaza `window.Notification` con `show_notification` IPC. `Notification.permission` devuelve `'granted'` permanentemente. `show_notification_internal` (Rust) elige según plataforma:
  - **Linux — notificación nativa** (`show_native_notification`, crate `notify-rust` vía D-Bus): el escritorio la ubica y respeta "No molestar"; click → `show_window` (o `show_window_2`). Las de la cuenta 2 llevan "(Cuenta 2)" en el título (el comando `show_notification` recibe la `WebviewWindow` que lo invocó y mira su label). Cada cuenta tiene su propio `NativeNotifSlot`; dentro de una cuenta todas reusan el mismo id (`replaces_id`), así se ve solo la última (como el toast) y alcanza con **un único thread** bloqueado en `wait_for_action` (cubre las notificaciones que reemplazan a la primera); al clickear o descartar, se resetea el id. Se pasó a nativas porque en Wayland las apps no pueden posicionar ventanas: el toast HTML salía centrado.
  - **Otras plataformas — toast HTML** (`show_toast_notification`): reusa la ventana `notification` entre toasts en vez de destruirla y recrearla: si ya existe, navega su contenido con `window.location.replace(...)` y la reposiciona/muestra; si no, la crea. `close_notification` y `open_whatsapp` la ocultan (`hide`) en vez de cerrarla. Un contador de generación (`NotificationGenState`) evita que el auto-cierre diferido (5,5 s) de un toast viejo oculte uno más nuevo que reusó la misma ventana.
  - **`target_monitor`**: el toast y la ventana de logs se ubican con `primary_monitor()`, que en Wayland devuelve `None` (no existe "monitor principal"). Fallback: monitor de la ventana `whatsapp-web`, o el primero disponible. Antes de este fallback el toast se descartaba sin loggear en Wayland.
- **Modelo de caché de WebKit:** `context.set_cache_model(CacheModel::DocumentViewer)` (en `create_whatsapp_window_impl`, junto a la corrección ortográfica). Desactiva la caché en memoria de recursos HTTP. Medido con A/B en uso real: ~1.210 MB → ~1.084 MB (~125 MB menos, ~10%) sin diferencia de fluidez notable — WhatsApp guarda sus datos en IndexedDB/service worker, no en esa caché. Los límites de memoria de WebKit (`MemoryPressureSettings`) no son configurables sin forkear wry: solo se pueden pasar al construir el `WebContext`, y eso lo hace wry internamente.
- **User agent (`chrome_user_agent` en `window.rs`):** Las ventanas de WhatsApp usan un UA de Chrome/Linux para que WhatsApp Web no detecte un navegador desconocido y bloquee el acceso o recorte funciones. La versión mayor de Chrome **se calcula por fecha** (131 el 2024-11-12, +1 cada 28 días, el ciclo de Chrome) para que no quede vieja entre releases: hasta v1.2.18 estaba fijo en Chrome 131, 25 versiones atrás. Un test (`user_agent_con_version_de_chrome_vigente`) verifica que dé ≥ 156 (estable real al 2026-10-28). Si Chrome cambia su ciclo de releases, ajustar `CYCLE_SECS`.
- **Segunda cuenta al inicio (arranque):** Si `account2_visible` está en `true`, `create_whatsapp_window_2` se lanza en un `std::thread::spawn` dentro de `setup()` en vez de llamarse sincrónicamente. Crear el segundo contexto WebKit ahí adentro bloquearía el armado del tray y el registro del atajo global (Win+Alt+W) hasta que termine. Al togglear la cuenta 2 desde el tray en caliente sí se llama sincrónico (ya no está en el path crítico de arranque).

---

#### Comportamientos Rust

**Cierre de ventana → hide (`on_window_event`):**
`on_window_event` con `CloseRequested` llama `api.prevent_close()` + `window.hide()`. La ventana de WhatsApp nunca se destruye; solo se oculta. Esto es intencional: destruirla y recrearla es lento y pierde la sesión si el proceso WebKit muere.

**`show_window`:** Llama `unminimize()` → `set_always_on_top(true)` → `show()` → `set_focus()` → (150 ms después, en otro thread) `set_always_on_top(false)`. El truco `always_on_top` es necesario en algunos WMs (ej. i3, algunos GNOME) que ignoran `set_focus()` sin que la ventana sea temporalmente top-level.

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
| Segunda cuenta WhatsApp | CheckMenuItem | toggle `Account2VisibleState`, crea/muestra/oculta ventana `whatsapp-web-2` |
| Descargar PDF abre con visor externo | CheckMenuItem | toggle `PdfExternalReaderState`, persiste en `config.json` (ver sección "PDFs") |
| Buscar actualización | MenuItem | `check_for_updates(app)` |
| Ver logs | MenuItem | abre/enfoca ventana `logs` |
| Acerca de | MenuItem | `show_about` (tray.rs): diálogo con la versión y botón "Abrir GitHub" (`GITHUB_URL` en constants.rs, se abre con `open_url_external`) |
| Salir | MenuItem | `app.exit(0)` |

El menú se reconstruye completamente en cada `update_tray_menu()` → `build_tray_menu()`.

**Badge de no leídos (Rust):**
`regenerate_tray_icon` dibuja un círculo rojo (radio 14, centro 48,16) con número pixel art (5×7, escala 2) sobre el RGBA base (64×64 px). Solo cuando `count > 0`. Muestra `9+` para counts > 9.

`draw_filled_circle` usa anti-aliasing manual via `blend_pixel`. Ningún crate de imágenes.

> **⚠ No tocar:** `TrayBadgeState.base_rgba` se captura una sola vez al inicio desde el PNG embebido; si se pierde, el badge no puede regenerarse. `FONT_SCALE = 2` calibra los dígitos para el ícono de 64×64.

---

#### Config persistente

`config.json` en `app.path().app_config_dir()` (Linux: `~/.config/whatajost/config.json`). Campos:
- `notifications_enabled` (bool, default `true`)
- `account2_visible` (bool, default `false`)
- `pdf_external_reader` (bool, default `false`)

Todas las escrituras usan `modify_config(app, |json| { ... })`: lee el JSON existente, aplica el closure, y escribe. Garantiza merge sin pisar otros campos y loggea errores de escritura. No hay migración de schema.

---

#### Auto-updater (funciona ✓)

`check_for_updates_impl(app, silent)` — corre en background thread.
- **Al inicio y cada 2 hs:** llamado con `silent=true` en loop — primer chequeo a los 5 s (para no interferir con la carga inicial de WhatsApp Web), después repite cada 2 hs mientras el proceso siga vivo. Necesario porque cerrar la ventana solo la oculta (no mata el proceso, ver "Cierre de ventana → hide" más abajo): sin este loop, un chequeo único al arrancar nunca se repite y una sesión de días/semanas no se entera de versiones nuevas.
- **Desde el menú:** llamado con `silent=false` (muestra diálogos de error si falla).

Flujo:
1. `tauri_plugin_updater` verifica `latest.json` en GitHub releases. Como no fijamos `.target(...)` al registrar el plugin, la resolución de URL/firma prueba primero `linux-x86_64-{bundle_type}` (con `bundle_type()` detectado en runtime vía el patching binario que hace `tauri-bundler` con `patchelf` en cada paquete) y si no existe esa clave cae a `linux-x86_64` genérico. Hoy `latest.json` solo define una clave específica extra para AppImage (`linux-x86_64-appimage`) — el `.deb` usa la genérica.
2. Si hay update: diálogo `OkCancel` con la versión nueva.
3. Descarga el paquete (`update.download(...)`).
4. **Linux, corriendo como AppImage** (detectado con `std::env::var_os("APPIMAGE")`, la variable que el propio runtime de AppImage setea): se usa `update.install(bytes)` — el instalador que ya trae `tauri-plugin-updater`, que hace un `rename` del AppImage actual a un backup temporal, escribe los bytes nuevos en el path original y restaura los permisos (evita "text file busy" porque nunca escribe sobre el inode que está corriendo). No necesita root. Si tiene éxito, `h.restart()` (que también es AppImage-aware: relanza el path de `$APPIMAGE`, no el binario montado en el `/tmp` de FUSE).
5. **Linux, instalado como .deb** (cualquier otro caso): se guarda el paquete a un temp file (`.deb`) y 4 intentos de instalación en cascada:
   - `pkexec apt install --yes <file>` (Ubuntu moderno, maneja dependencias)
   - `pkexec dpkg -i <file>` (fallback sin resolver deps)
   - `zenity --password` + `sudo -S dpkg -i <file>` (fallback sin pkexec)
   - `xdg-open <file>` (abre el instalador gráfico del sistema; muestra aviso al usuario para que complete manualmente)
6. **Windows:** `msiexec /i <file> /quiet`.
7. Si la instalación fue exitosa (deb o Windows): `h.restart()`.

> **⚠ No tocar:** el orden de los 4 intentos Linux del paso 5 (el último abre un diálogo y retorna, no espera a que el usuario instale — si se invierte el orden, los intentos con pkexec no corren). El chequeo de `APPIMAGE` tiene que ir *antes* de escribir el temp file `.deb` — si un usuario de AppImage cayera en la cascada pkexec/dpkg, fallaría siempre (no es un paquete Debian).

---

### Comandos Tauri (IPC)

| Comando | Descripción |
|---|---|
| `save_file_begin` | Crea el archivo temporal de una descarga y devuelve su `id` |
| `save_file_chunk(id, data)` | (async) Decodifica un trozo base64 y lo agrega al temporal |
| `save_file_abort(id)` | Borra el temporal de una descarga fallida |
| `save_file(id, file_name)` | Abre `rfd::FileDialog` y mueve el temporal ahí; si el diálogo no aparece (xdg-portal), lo mueve a `~/Downloads/WhataJOST/` y lo registra en logs |
| `read_clipboard_image` | Lee imagen del portapapeles del sistema con `wl-paste` o `xclip` (detecta herramienta disponible con `OnceLock`) |
| `read_file_for_drop(path)` | Lee un archivo local por path (usado desde el DOM drop handler cuando `dataTransfer.files` está vacío en Linux) |
| `update_unread_count(count)` | Renderiza badge numérico sobre el ícono de bandeja (pixel art en RGBA, sin librerías de imágenes) |
| `show_notification(title, body)` | Linux: notificación nativa D-Bus. Otras: reusa (o crea) la ventana `notification` (HTML transparente, always-on-top) que se auto-oculta a los 5,5 s |
| `close_notification` | Oculta la ventana de notificación (no la destruye, para reusarla) |
| `open_whatsapp` | Muestra y enfoca la ventana principal, oculta la notificación |
| `open_external(url)` | Abre en el navegador/app del sistema un link que el usuario clickeó (solo `http`, `https`, `mailto`, `tel`) |
| `log_js(level, message)` | Recibe logs del script JS y los agrega al buffer en memoria (capado a `MAX_LOG_ENTRIES` = 500, `VecDeque`) |
| `get_logs` | Devuelve todos los logs del buffer (usada por `logs.html`) |
| `clear_logs` | Vacía el buffer de logs |
| `open_logs` | Abre la ventana del visor de logs centrada en pantalla |
| `get_resource_usage` | CPU % y memoria (PSS) de la app y sus procesos hijos, leído de `/proc` (solo Linux; si no, `available: false`) |

### Capabilities (IPC por ventana)

| Archivo | Ventana | Permisos |
|---|---|---|
| `whatsapp-notification.json` | `whatsapp-web`, `whatsapp-web-2` (remotas: `*.whatsapp.com`, `*.whatsapp.net`) | `core:default`, `allow-whatsapp-ipc` |
| `logs-window.json` | `logs` | `core:default`, `allow-whatsapp-ipc` |
| `notification-window.json` | `notification` | `core:default`, `allow-whatsapp-ipc` |

La ventana `whatsapp-web` carga una URL externa; el IPC solo funciona cuando la URL coincide con los patrones de `remote.urls`.

**Importante — ACL de Tauri 2.11.1+**: Desde Tauri 2.11.1, todos los comandos custom de la app (no solo plugins) requieren permiso ACL explícito cuando son invocados desde webviews remotos. El archivo `src-tauri/permissions/default.toml` define el permiso `allow-whatsapp-ipc` que lista todos los comandos IPC de la app. Sin ese archivo (o sin incluir `allow-whatsapp-ipc` en el capability de la ventana remota), los comandos fallan silenciosamente — no aparece ningún error en la consola JS ni en los logs de Rust.

### Ventanas HTML (`public/`)

- `logs.html` — Dos pestañas:
  - **Logs**: visor en tiempo real (polling 2 s a `get_logs`); muestra error explícito si IPC no está disponible.
  - **Recursos** (solo Linux): gráficos de CPU y memoria de los últimos 5 min (150 puntos, dibujados a mano en `<canvas>`, sin librerías) y tabla de procesos. Llama `get_resource_usage` cada 2 s **solo mientras la ventana está abierta** (con cualquier pestaña visible, para que el historial ya esté armado): con la ventana cerrada no se mide nada. `get_resource_usage` recorre `/proc` para armar el árbol de procesos desde el propio pid (whatajost + WebKit web/red/GPU + sandbox), suma `utime+stime` (USER_HZ = 100, fijo en la ABI) y calcula el % por diferencia contra la lectura anterior (estilo `top`: 100 % = 1 núcleo); memoria = PSS de `smaps_rollup`. Fuera de Linux devuelve `available: false` y la pestaña muestra "Disponible solo en Linux".
- `notification.html` — Toast de notificación transparente (se auto-cierra a los 5,5 s). Solo fuera de Linux.

`tauri.conf.json` usa `frontendDist: "../public"` directo, sin bundler, y `app.windows: []`. Antes había una ventana `main` oculta de 1×1 px con un `index.html` vacío (resto de la plantilla) que costaba un proceso WebKit entero (~35 MB) sin uso, y Vite solo existía para compilar esa página: se eliminaron ambos. La app no se cierra por no tener esa ventana: `whatsapp-web` nunca se destruye (se oculta).

## Key dependencies

- `rfd` v0.17 con features `xdg-portal` + `wayland` — diálogos de archivo nativos
- `tauri-plugin-dialog` con feature `xdg-portal` — diálogos de mensaje nativos (updater, errores)
- Se quitaron `tauri-plugin-clipboard-manager` y `tauri-plugin-process` (registrados pero sin uso, 24 crates menos): el portapapeles se lee con `wl-paste`/`xclip`, "Copiar imagen" usa la Clipboard API del navegador (habilitada por `connect_permission_request`), y el reinicio del updater es `AppHandle::restart` de Tauri core
- `webkit2gtk` v2 (Linux) — corrección ortográfica + devtools en producción
- `glib` v0.18 + `gtk` v0.18 (Linux) — señales GTK para interceptar drag & drop a nivel nativo (ya eran transitivas vía webkit2gtk; se agregan como deps directas para usar `WidgetExt::connect_drag_data_received`)
- `mime_guess` — detección de MIME type para archivos arrastrados desde el SO
- `notify-rust` v4 (Linux) — notificaciones nativas por D-Bus (usa `zbus` 5, que ya venía por `rfd`)
- `wl-clipboard` (`wl-paste`) / `xclip` — lectura de imágenes del portapapeles del sistema (dependencias `.deb`)

### Bug de botones de ventana (min/max/cerrar) en Wayland — resuelto en tao ≥ 0.36

tao 0.35.3 tenía un bug de CSD en Wayland ([tauri-apps/tauri#11856](https://github.com/tauri-apps/tauri/issues/11856), [tauri-apps/tao#1046](https://github.com/tauri-apps/tao/issues/1046)): tras ocultar/mostrar la ventana, los botones minimizar/maximizar/cerrar dejaban de responder al click. Reportado en Ubuntu 26/GNOME 48+. El fix ([tao#1218](https://github.com/tauri-apps/tao/pull/1218)) se publicó en tao 0.36.0; durante un tiempo `Cargo.toml` tuvo un `[patch.crates-io]` apuntando a la rama `dev` de tao, que se quitó al pasar a Tauri 2.12 (tao 0.37).

**Si el bug vuelve a aparecer**, revisar primero la versión de tao en `Cargo.lock` antes de tocar `show_window`. El hack de resize de 1px que tenía `show_window` (mitigación propia previa al fix de tao) se quitó en v1.2.9 tras probar en Wayland que con tao 0.37 ya no hace falta.

## Release

Para publicar una nueva versión:

```bash
./release.sh <version>
# Ejemplo: ./release.sh 1.1.34
```

El script:
0. Corre los chequeos (`node --check` de `inject.js` + `cargo clippy -- -D warnings`); si fallan, aborta antes de tocar nada
1. Actualiza la versión en `package.json`, `src-tauri/Cargo.toml` y `src-tauri/tauri.conf.json`
2. Ejecuta `cargo update` y `pnpm update`
3. Commitea los cambios de versión y lock files (`Release v<version>`)
4. Crea y pushea el tag `v<version>` a GitHub y git.jost.ar

El build y la publicación del release (`.deb`, `.AppImage`, `.msi`, `.sig`, `latest.json`). **No se genera `.rpm`** desde v1.2.15: el auto-update de una instalación .rpm nunca funcionó (bajaba el `.deb` de la clave genérica), así que en Fedora/Arch/etc. se usa el AppImage los hace GitHub Actions al detectar el tag — no corren localmente. `release.yml` primero llama a `check.yml` (job `check`) y solo si pasa arranca el build; `check.yml` también corre solo en cada push a `main` y en PRs. Clippy corre en Linux: el código `cfg(not(target_os = "linux"))` (toast HTML, etc.) solo se compila en el build de Windows, sin `-D warnings`. Seguir el progreso en `https://github.com/marceloabelda/whataJOST/actions`.

Requisitos:
- Estar en la rama `main` sin cambios pendientes
- `cargo` en el PATH (`. "$HOME/.cargo/env"` si la shell no lo carga) y `node`
