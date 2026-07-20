# WhataJOST

Wrapper nativo de WhatsApp Web construido con [Tauri v2](https://v2.tauri.app/).

Fork de [jude7733/whatauri](https://github.com/jude7733/whatauri).

```
                        ┌──────────────────────────────────┐
                        │           WhataJOST              │
                        └──────────────┬───────────────────┘
                                       │
                                       ▼
                            ┌─────────────────┐
                            │  WhatsApp Web   │
                            │                 │
                            │  mensajes       │
                            │  archivos       │
                            │  paste/drop     │
                            │  notificaciones │
                            │  deep links     │
                            └────────┬────────┘
                                     │
                          ┌──────────▼─────────┐
                          │     System Tray     │
                          │                     │
                          │   [ico]  [N msgs]   │
                          └─────────────────────┘
```

---

## Características

### WhatsApp Web

- Carga en segundo plano, sin pantalla en blanco al abrir
- User agent Chrome: carga la versión completa de WhatsApp Web
- Instancia única: una sola ventana abierta a la vez
- Cierre de ventana oculta la app (no la termina), preservando la sesión
- Versión visible en la barra de título
- Atajo de teclado global **Ctrl+Shift+W** para mostrar/ocultar la ventana

### Bandeja del sistema (tray)

- Click izquierdo: alterna mostrar/ocultar la ventana
- Click derecho: menú contextual
- Badge numérico con mensajes sin leer sobre el ícono
- Toggle de notificaciones emergentes (persistido en disco)
- Inicio automático con el sistema (autostart)

### Notificaciones

- Notificaciones in-app al recibir mensajes (toast estilo Telegram con nombre y preview)
- Toggle para activar/desactivar notificaciones emergentes desde el menú del tray

### Archivos y portapapeles

- Pegar imágenes desde el portapapeles del sistema (Wayland y X11)
- Drag & drop de archivos desde el explorador de archivos
- Descarga de archivos (PDF, imágenes, etc.) con diálogo nativo de guardado

### Links y deep links

- Links externos se abren en el navegador del sistema
- Deep links `whatsapp://` — se puede configurar WhataJOST como handler del sistema para abrir chats desde el browser o desde otras apps

### Actualizaciones

- Actualización automática al iniciar (en segundo plano, sin interrumpir)
- Verificación manual desde el menú del tray
- Reinicio automático post-actualización

---

## Instalación

Descargá el `.deb` desde la [página de releases](../../releases/latest) e instalalo con:

```bash
sudo dpkg -i whatajost_*.deb
```

O abrí el archivo `.deb` con el instalador de paquetes de tu escritorio.

---

## Créditos

Ícono: [WhatsApp icon by Icons8](https://icons8.com/icon/42961/whatsapp)
