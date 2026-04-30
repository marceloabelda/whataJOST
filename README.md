# WhataJOST

WhataJOST is a whatsapp webview client built with [Tauri](https://v2.tauri.app/).

es un fork de https://github.com/jude7733/whatauri



## TODO

- [x] System tray
- [x] Performance
- [x] Single instance (una sola ventana abierta a la vez)
- [x] Abrir links externos en el browser del sistema
- [x] User agent Chrome (carga la versión completa de WhatsApp Web)
- [x] Carga en segundo plano (sin pantalla en blanco al abrir)
- [x] Notificaciones in-app al recibir mensajes (toast estilo Telegram con nombre y preview)
- [x] Actualización automática (checkea al iniciar y desde el menú del tray)
- [x] Pegar imágenes desde el portapapeles del sistema
- [x] Versión visible en la barra de título
- [ ] Descargar archivos (PDF, imagenes.. etc)
- [ ] Auto Startup
- [ ] Atajo de teclado global (mostrar/ocultar ventana)
- [ ] Recordar tamaño y posición de ventana
- [ ] Badge con mensajes sin leer en el tray
- [ ] Múltiples cuentas
- [ ] Zoom configurable
- [ ] Modo no molestar
- [ ] Click en tray icon, siempre trae la app a primer plano
- [ ] Agregar el el tray, Minimize to tray


## Installation

### Debian / Ubuntu

```bash
sudo dpkg -i whataJOST_*_amd64.deb
```

### Fedora

```bash
sudo rpm -i whataJOST-*.x86_64.rpm
```

### Desde código

```bash
git clone https://github.com/marceloabelda/whataJOST.git
cd whataJOST
pnpm install
pnpm tauri dev
```

## Actualización automática

La app chequea actualizaciones automáticamente al iniciar (en silencio, solo notifica si hay una nueva versión). También podés buscar manualmente desde el ícono del systray → **Buscar actualización**.

Para publicar una nueva versión:

```bash
./release.sh 1.2.0
```

El script buildena, firma los paquetes, genera el manifiesto y crea el release en GitHub.

## Icon

<a target="_blank" href="https://icons8.com/icon/42961/whatsapp">WhatsApp</a>
