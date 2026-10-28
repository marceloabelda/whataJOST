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
// Badge (icono del tray con número)
// ============================================================================
/// Centro X del círculo de badge en el ícono del tray (64x64 px)
pub const TRAY_BADGE_CENTER_X: u32 = 48;

/// Centro Y del círculo de badge en el ícono del tray (64x64 px)
pub const TRAY_BADGE_CENTER_Y: u32 = 16;

/// Radio del círculo de badge en píxeles
pub const TRAY_BADGE_RADIUS: f32 = 14.0;

/// Escala de fuente para números de badge
/// (calibrada para ícono 64x64, dígitos 5x7 pixels base)
pub const TRAY_BADGE_FONT_SCALE: u32 = 2;

/// Color rojo del badge (RGB, sin alpha)
pub const TRAY_BADGE_RED: u8 = 255;
pub const TRAY_BADGE_GREEN: u8 = 0;
pub const TRAY_BADGE_BLUE: u8 = 0;

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
