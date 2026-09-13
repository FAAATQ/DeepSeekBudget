// Release builds must not open a console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! API Budget — see the price, know when to wait.
//!
//! A menu bar indicator for DeepSeek's peak/off-peak API pricing. There is no main window:
//! the app is a tray icon plus a popover created on demand. The scheduling logic lives in
//! the `apibudget-schedule` crate so it can be tested without any of this.

mod core;
mod popover;
mod provider;
mod tick;
mod tray;

use std::sync::Mutex;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // No Dock icon and no Cmd-Tab entry: this is a menu bar accessory, not an app you
            // switch to. Without this macOS gives it a Dock tile and steals focus.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            app.manage(Mutex::new(core::AppCore::load(settings_dir)));

            tray::build(app)?;
            tick::spawn(app.handle().clone());

            // Development affordance, same spirit as APIBUDGET_FAKE_NOW: the popover is
            // created lazily on the first tray click, which is the only moment the native
            // window material gets applied — so without this, checking that the glass call
            // succeeds (and that the CSS fallback flag is set correctly) would require a
            // human to click a menu bar icon.
            if std::env::var(popover::OPEN_POPOVER_ENV)
                .is_ok_and(|value| !value.trim().is_empty() && value.trim() != "0")
            {
                popover::show_at_tray(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            core::get_view,
            core::get_settings,
            core::get_environment,
            core::get_provider_status,
            core::set_currency,
            core::set_timezone_offset,
            core::set_language,
            core::apply_provider_config,
            core::reset_provider_config,
            core::hide_popover,
        ])
        .on_window_event(|window, event| {
            if window.label() != popover::LABEL {
                return;
            }
            match event {
                // Clicking anywhere else dismisses the popover.
                tauri::WindowEvent::Focused(false) => {
                    // `note_focus_lost` decides whether this is a dismissal at all: a panel
                    // that never held focus has nothing to lose, and hiding it would close a
                    // panel the user never saw. It also records the time, because on Windows
                    // the click on the tray icon blurs the panel *before* it arrives as a tray
                    // event — `toggle` needs to know the two are the same gesture.
                    if popover::note_focus_lost() {
                        let _ = window.hide();
                    }
                }
                tauri::WindowEvent::Focused(true) => popover::note_focus_gained(),
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("API Budget failed to start");
}
