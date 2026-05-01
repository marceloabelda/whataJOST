// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Fix intermitent emoji rendering in WebKitGTK (WhatsApp Web emoji picker)
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    whatajost_lib::run()
}
