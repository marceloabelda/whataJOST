# WhataJOST

WhatsApp Web para equipos de infraestructura. Wrapper nativo de WhatsApp Web construido con [Tauri v2](https://v2.tauri.app/), con integraciones de monitoreo para Uptime Kuma y Zabbix.

Fork de [jude7733/whatauri](https://github.com/jude7733/whatauri).

```
                        ┌──────────────────────────────────┐
                        │           WhataJOST              │
                        │   WhatsApp Web para infra        │
                        └──────────────┬───────────────────┘
                                       │
           ┌───────────────────────────┼──────────────────────────┐
           │                           │                          │
           ▼                           ▼                          ▼
  ┌─────────────────┐       ┌──────────────────┐       ┌──────────────────┐
  │  WhatsApp Web   │       │   Uptime Kuma    │       │     Zabbix       │
  │                 │       │                  │       │                  │
  │  mensajes       │       │  GET /metrics    │       │  POST JSON-RPC   │
  │  archivos       │       │  cada 30 s       │       │  cada 30 s       │
  │  paste/drop     │       │  ● verde / rojo  │       │  ● verde / rojo  │
  │  notificaciones │       │  notifica cambio │       │  por severidad   │
  │  deep links     │       │  acknowledged    │       │  acknowledged    │
  └─────────────────┘       └────────┬─────────┘       └────────┬─────────┘
                                     └──────────┬───────────────┘
                                                │
                                   ┌────────────▼─────────────┐
                                   │       System Tray        │
                                   │                          │
                                   │  [ico] [● UK] [● ZBX] [N]│
                                   │         ↑       ↑     ↑  │
                                   │       Uptime  Zabbix badge│
                                   │       Kuma    probl.  msgs│
                                   │                          │
                                   │  ┌──────────────────────┐│
                                   │  │    Barra flotante    ││
                                   │  │  ●s1 ●s2 │ ✗prob1   ││
                                   │  └──────────────────────┘│
                                   └──────────────────────────┘
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
- Puntos de estado de Uptime Kuma y Zabbix integrados en el ícono del tray
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

## Monitoreo de infraestructura

### Uptime Kuma

- Polling al endpoint `/metrics` cada 30 segundos
- Estado visible en el ícono del tray: punto verde (todo ok), rojo (monitor caído), naranja (sin conexión)
- Submenu en el tray con el estado de cada monitor (`✓/✗ nombre`)
- Notificación cuando un monitor cambia de estado
- Configuración desde el menú del tray (URL + API Key)

### Zabbix

- Polling a la API JSON-RPC de Zabbix cada 30 segundos
- Filtro por severidad configurable (No clasificado / Información / Advertencia / Promedio / Alto / Desastre)
- Estado visible en el ícono del tray: punto verde (sin problemas), rojo (hay problemas activos), naranja (sin conexión)
- Submenu en el tray con los problemas activos (`✗ [Severidad] nombre`)
- Notificación cuando aparece un problema nuevo o se resuelve uno existente
- Ignorar problemas acknowledged
- Configuración desde el menú del tray (URL + API Token + severidades)

### Barra flotante de monitoreo

Ventana pequeña siempre visible (420×44 px) que muestra el estado de Uptime Kuma y Zabbix en tiempo real. Se activa desde el menú del tray → **Barra flotante**.

- Pill semitransparente con glassmorphism
- Puntos coloreados por estado con tooltip al pasar el mouse
- Se puede arrastrar a cualquier posición de la pantalla
- Se sincroniza con el estado del tray

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
