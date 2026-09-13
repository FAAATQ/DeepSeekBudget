//! The menu bar icon and its right-click menu.

use apibudget_schedule::PriceState;
use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, Manager, Position, Size,
};

use crate::core::{AppCore, APP_NAME};

const ICON_OFF_PEAK: &[u8] = include_bytes!("../icons/tray-offpeak.png");
const ICON_PEAK: &[u8] = include_bytes!("../icons/tray-peak.png");
const ICON_UNKNOWN: &[u8] = include_bytes!("../icons/tray-unknown.png");

pub const TRAY_ID: &str = "main";

/// Decode the bundled icon for a state.
///
/// Decoding per update costs microseconds and keeps lifetimes simple. A decode failure is
/// non-fatal: the tray keeps whatever icon it already had.
pub fn icon_for(state: Option<PriceState>) -> Option<Image<'static>> {
    let bytes = match state {
        Some(PriceState::OffPeak) => ICON_OFF_PEAK,
        Some(PriceState::Peak) => ICON_PEAK,
        None => ICON_UNKNOWN,
    };
    Image::from_bytes(bytes)
        .map_err(|e| eprintln!("{APP_NAME}: bundled tray icon failed to decode: {e}"))
        .ok()
}

pub fn apply_icon(tray: &tauri::tray::TrayIcon, state: Option<PriceState>) {
    let Some(icon) = icon_for(state) else { return };

    #[cfg(target_os = "macos")]
    {
        // `set_icon` followed by `set_icon_as_template` renders the icon twice and visibly
        // flickers, so use the combined call. The `false` keeps colour, which is the entire
        // point of having three icons — template mode renders only the alpha channel as a
        // monochrome mask, throwing the blue/orange/grey semantics away.
        let _ = tray.set_icon_with_as_template(Some(icon), false);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = tray.set_icon(Some(icon));
    }
}

pub fn build(app: &App) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "About API Budget", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit API Budget", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&settings, &about, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(APP_NAME)
        .menu(&menu)
        // Open the popover on left-click; keep the menu for right-click. Without this the
        // menu would pop up on both, which makes the popover unreachable.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "settings" => crate::popover::show_settings(app),
            "about" => crate::popover::show_about(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // `TrayIconEvent` is #[non_exhaustive], so match with a catch-all arm.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                crate::popover::toggle(tray.app_handle(), rect);
            }
        });

    // Set the correct icon immediately, so launch does not flash the wrong colour before the
    // first tick lands.
    if let Some(icon) = icon_for(current_state(app)) {
        builder = builder.icon(icon);
    }

    let tray = builder.build(app)?;
    report_rect_if_asked(&tray);
    Ok(())
}

/// Development affordance, same spirit as `APIBUDGET_FAKE_NOW` and `APIBUDGET_OPEN_POPOVER`:
/// print where the shell says this tray icon actually is.
///
/// This exists because guessing the coordinates does not work. A colour search over a taskbar
/// full of other icons cannot say which slot is ours — it produced two measurements of the
/// same pixels that flatly disagreed with each other, and both were wrong. Asking the app
/// costs a few lines and is exact, which is what makes the click path testable by script.
fn report_rect_if_asked(tray: &tauri::tray::TrayIcon) {
    if std::env::var("APIBUDGET_REPORT_TRAY_RECT").is_err() {
        return;
    }

    match tray.rect() {
        Ok(Some(rect)) => match (rect.position, rect.size) {
            (Position::Physical(position), Size::Physical(size)) => eprintln!(
                "{APP_NAME}: tray rect x={} y={} w={} h={} centre=({},{})",
                position.x,
                position.y,
                size.width,
                size.height,
                position.x + size.width as i32 / 2,
                position.y + size.height as i32 / 2
            ),
            _ => eprintln!("{APP_NAME}: tray rect is not in physical pixels: {rect:?}"),
        },
        Ok(None) => eprintln!(
            "{APP_NAME}: the shell reports no rect for this tray icon — it is probably in the \
             overflow area"
        ),
        Err(e) => eprintln!("{APP_NAME}: could not read the tray rect: {e}"),
    }
}

fn current_state(app: &App) -> Option<PriceState> {
    let core = app.state::<Mutex<AppCore>>();
    // Bind the guard to a local so it is dropped at the end of this statement, rather than
    // as a tail-expression temporary that outlives the `State` it borrows from.
    let state = match core.lock() {
        Ok(guard) => guard.view().state,
        Err(poisoned) => poisoned.into_inner().view().state,
    };
    state
}
