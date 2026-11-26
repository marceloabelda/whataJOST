//! Constantes de configuración y comportamiento de la aplicación

// ============================================================================
// Logging
// ============================================================================
/// Máximo número de entradas en el buffer de logs en memoria
pub const MAX_LOG_ENTRIES: usize = 500;

// ============================================================================
// Ventanas
// ============================================================================
/// Delay en ms antes de quitar `always_on_top` después de mostrar la ventana
/// (algunos WMs ignoran `set_focus()` si la ventana no es top-level un instante)
pub const GTK_WINDOW_RESTORE_DELAY_MS: u64 = 150;

/// Delay en ms inicial antes del primer chequeo de actualizaciones
/// (permite que WhatsApp Web cargue sin interrupciones)
pub const FIRST_UPDATE_CHECK_DELAY_MS: u64 = 5000;

/// Intervalo en segundos entre chequeos de actualizaciones
pub const UPDATE_CHECK_INTERVAL_SECS: u64 = 2 * 60 * 60; // 2 horas

// ============================================================================
// Notificaciones emergentes
// ============================================================================
/// Tiempo en ms que la ventana de notificación permanece visible
pub const NOTIFICATION_DISPLAY_TIME_MS: u64 = 5500;

// ============================================================================
// Acerca de
// ============================================================================
/// Repositorio del proyecto (diálogo "Acerca de" del tray)
pub const GITHUB_URL: &str = "https://github.com/marceloabelda/whataJOST";

// ============================================================================
// Badge (icono del tray con número)
// ============================================================================
/// Centro X del círculo de badge de la cuenta principal (rojo, a la derecha)
/// en el ícono del tray (64x64 px)
pub const TRAY_BADGE_CENTER_X: u32 = 48;

/// Centro X del círculo de badge de la cuenta 2 (amarillo, a la izquierda).
/// Mismo Y y radio que el principal.
pub const TRAY_BADGE2_CENTER_X: u32 = 16;

/// Centro Y del círculo de badge en el ícono del tray (64x64 px)
pub const TRAY_BADGE_CENTER_Y: u32 = 16;

/// Radio del círculo de badge en píxeles
pub const TRAY_BADGE_RADIUS: f32 = 14.0;

/// Escala de fuente para números de badge
/// (calibrada para ícono 64x64, dígitos 5x7 pixels base)
pub const TRAY_BADGE_FONT_SCALE: u32 = 2;

/// Badge de la cuenta principal: círculo rojo, número blanco (RGB)
pub const TRAY_BADGE_COLOR: [u8; 3] = [255, 0, 0];
pub const TRAY_BADGE_TEXT: [u8; 3] = [255, 255, 255];

/// Badge de la cuenta 2: círculo amarillo, número oscuro (el blanco no se
/// lee sobre amarillo)
pub const TRAY_BADGE2_COLOR: [u8; 3] = [255, 200, 0];
pub const TRAY_BADGE2_TEXT: [u8; 3] = [30, 30, 30];

// ============================================================================
// Drag & drop
// ============================================================================
/// Ventana en segundos durante la cual una ruta arrastrada de forma nativa
/// puede ser leída por los fallbacks que dispara la página
pub const NATIVE_DROP_ALLOW_SECS: u64 = 30;

/// Tamaño de cada trozo al inyectar un archivo arrastrado en la página
/// (cada trozo viaja como un `eval` con ~4 MB de base64)
pub const DROP_CHUNK_BYTES: usize = 3 * 1024 * 1024;

// ============================================================================
// URLs y hosts
// ============================================================================
/// URL base de WhatsApp Web
pub const WHATSAPP_WEB_URL: &str = "https://web.whatsapp.com/";
