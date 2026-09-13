//! The panel that opens under the tray icon.
//!
//! Created lazily on first use: an app that lives in the menu bar and has never been clicked
//! should not be paying for a webview process.
//!
//! Known V0.1 compromise: this is a plain always-on-top window, so opening it activates the
//! app and takes focus from whatever was in front. Spotlight-style non-activating behaviour
//! needs an `NSPanel` with the non-activating style mask, which in Tauri means a git-only
//! third-party plugin depending on the deprecated `objc`/`cocoa` crates. Not a dependency
//! worth taking for v0.1 of a tool whose pitch is a small, robust binary.

use crate::core::{AppCore, APP_NAME};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, Rect, Size, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const LABEL: &str = "popover";

/// Set this to open the popover at launch instead of waiting for a tray click. Development
/// only — see the note in `main.rs`.
pub const OPEN_POPOVER_ENV: &str = "APIBUDGET_OPEN_POPOVER";

/// Roughly the width the design settled on: wide enough for the day list and the price
/// table, narrow enough to read as a menu bar accessory rather than a window.
pub const WIDTH: f64 = 320.0;
pub const HEIGHT: f64 = 470.0;

/// The panel's corner radius, passed to the macOS glass APIs and mirrored by `--card-radius`
/// in `ui/styles.css`. The native view draws its own rounded corner, so a mismatch between the
/// two shows up as a visible halo where they disagree.
///
/// **This constant does not govern Windows, and `.card` must not round there at all.** The
/// Windows acrylic call takes no radius, so the material is a plain rectangle and the corner
/// comes from DWM, which rounds every top-level window to its own radius. An earlier version of
/// this comment claimed the opposite — that CSS was the only thing rounding a transparent,
/// undecorated Windows window — and the result was two different radii on the same corner: the
/// OS curve cutting across the card's curve, with the acrylic showing in the sliver between
/// them. `styles.css` now zeroes `--card-radius` when `data-platform="windows"` so only DWM's
/// curve is drawn.
///
/// Unused off macOS, hence the `cfg_attr` rather than a blanket `allow`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const CARD_RADIUS: f64 = 14.0;

/// Apply the native window material, preferring the newest API available.
///
/// Returns whether one actually applied. That answer is load-bearing: the CSS switches to a
/// translucent card only when a material is really behind it, because 10pt text over an
/// unblurred wallpaper is unreadable. Claiming glass we do not have would be worse than not
/// having it.
fn apply_window_material(window: &WebviewWindow) -> bool {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{LiquidGlassOptions, NSGlassEffectViewStyle};

        // macOS 26+: real Liquid Glass, via NSGlassEffectView.
        if window_vibrancy::apply_liquid_glass(
            window,
            LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular).radius(CARD_RADIUS),
        )
        .is_ok()
        {
            return true;
        }

        // Older macOS: the long-standing NSVisualEffectView material.
        return window_vibrancy::apply_vibrancy(
            window,
            window_vibrancy::NSVisualEffectMaterial::Popover,
            None,
            Some(CARD_RADIUS),
        )
        .is_ok();
    }

    #[cfg(target_os = "windows")]
    {
        return window_vibrancy::apply_acrylic(window, Some((18, 18, 20, 140))).is_ok();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = window;
        false
    }
}

fn ensure(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Some(window);
    }

    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(APP_NAME)
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .visible(false)
        .focused(false)
        .build()
        .map_err(|e| eprintln!("{APP_NAME}: could not create the popover: {e}"))
        .ok()?;

    let glass = apply_window_material(&window);
    if let Some(core) = app.try_state::<Mutex<AppCore>>() {
        match core.lock() {
            Ok(mut core) => core.glass_applied = glass,
            Err(poisoned) => poisoned.into_inner().glass_applied = glass,
        }
    }
    // Printed once per run, when the popover is first created. A GUI app's stderr goes
    // nowhere for normal users, but this is the only way to tell whether the native material
    // actually took — the answer decides the whole look of the panel.
    eprintln!(
        "{APP_NAME}: popover material — {}",
        if glass {
            "native glass applied"
        } else {
            "none available, falling back to an opaque background"
        }
    );

    Some(window)
}

/// When the popover last hid itself because it lost focus.
///
/// This exists for one platform-specific reason. `main.rs` hides the popover whenever it
/// loses focus — that is how "click anywhere else to dismiss" works. On Windows a click on
/// the tray icon blurs the popover *before* the tray event is delivered, so the two handlers
/// run in an order macOS never produces: focus-loss hides the panel, and the click that
/// caused it then sees a hidden panel and reopens it. Net effect — the icon that opened the
/// panel cannot close it.
///
/// A click arriving within [`REOPEN_GRACE`] of a focus-loss hide *is* that dismissal, not a
/// request to reopen. On macOS the guard is inert in practice, because clicking the menu bar
/// does not blur the panel the same way.
static LAST_FOCUS_HIDE: Mutex<Option<Instant>> = Mutex::new(None);

const REOPEN_GRACE: Duration = Duration::from_millis(250);

/// Whether the popover has actually held focus since it was last shown.
///
/// The focus-loss handler doubles as "click anywhere else and the panel goes away" — which
/// only means anything if the panel had focus to lose. On Windows a panel shown for a reason
/// other than a user gesture (the `APIBUDGET_OPEN_POPOVER` launch path, for example) can be
/// refused focus by the foreground lock, and then the first `Focused(false)` closes a panel
/// nobody ever saw. Measured on Windows: the panel was shown and gone again inside 700ms.
///
/// Gating on "did it ever hold focus" leaves it open in that case. On macOS the panel takes
/// focus when it is opened, so the gate is transparent there.
static HELD_FOCUS: AtomicBool = AtomicBool::new(false);

/// The popover gained focus, so losing it is a genuine dismissal.
pub fn note_focus_gained() {
    HELD_FOCUS.store(true, Ordering::Relaxed);
}

/// The popover lost focus. Returns whether that loss should dismiss it.
pub fn note_focus_lost() -> bool {
    // `swap` rather than `load`, so a second `Focused(false)` cannot dismiss twice.
    let had_focus = HELD_FOCUS.swap(false, Ordering::Relaxed);
    if had_focus {
        // Only recorded for a real dismissal: `toggle` uses it to tell apart the click that
        // closed the panel from a later click that means "open it again".
        if let Ok(mut last) = LAST_FOCUS_HIDE.lock() {
            *last = Some(Instant::now());
        }
    }
    had_focus
}

fn hid_just_now() -> bool {
    LAST_FOCUS_HIDE
        .lock()
        .ok()
        .and_then(|last| *last)
        .is_some_and(|at| at.elapsed() < REOPEN_GRACE)
}

pub fn toggle(app: &AppHandle, rect: Rect) {
    let Some(window) = ensure(app) else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else if hid_just_now() {
        // Still arriving from the gesture that just dismissed the panel. Do nothing: this
        // click is the one that closed it.
    } else {
        position_under(&window, rect);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Open the popover from a menu item, which has no click rect of its own — anchor it under
/// the tray icon instead.
pub fn show_at_tray(app: &AppHandle) {
    let rect = app
        .tray_by_id(crate::tray::TRAY_ID)
        .and_then(|tray| tray.rect().ok().flatten());
    let Some(window) = ensure(app) else { return };
    if let Some(rect) = rect {
        position_under(&window, rect);
    }
    let _ = window.show();
    let _ = window.set_focus();
    report_rect_if_asked(&window);
}

/// Development affordance, sharing `APIBUDGET_REPORT_TRAY_RECT` with the tray: print where the
/// popover actually ended up.
///
/// This exists for the same reason the tray counterpart does. Verifying the popover's rounded
/// corners means sampling its edge pixels, and sampling the wrong rectangle produces numbers
/// that look like a finding but are only a wrong guess about where the window is — which is
/// exactly what happened the first time this was measured by hand.
fn report_rect_if_asked(window: &WebviewWindow) {
    if std::env::var("APIBUDGET_REPORT_TRAY_RECT").is_err() {
        return;
    }

    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        eprintln!("{APP_NAME}: could not read the popover rect");
        return;
    };
    eprintln!(
        "{APP_NAME}: popover rect x={} y={} w={} h={} scale={:?}",
        position.x,
        position.y,
        size.width,
        size.height,
        window.scale_factor()
    );
}

pub fn show_settings(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "open-settings", ());
    show_at_tray(app);
}

pub fn show_about(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "open-about", ());
    show_at_tray(app);
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// Clearance between the panel and the icon, and between the panel and the edge of the work
/// area. Logical pixels; scaled before use. One constant serves both because nothing has ever
/// needed them to differ — the panel just has to not touch either edge.
const GAP: f64 = 6.0;

/// A rectangle in physical pixels. Mirrors what the tray event and `Monitor::work_area`
/// hand us, without dragging a window into the arithmetic — see [`panel_origin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    fn right(&self) -> i32 {
        self.x + self.width
    }

    fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

/// Where to put a `panel`-sized window, given the tray icon's rect and the **work area** of
/// the monitor that icon is on.
///
/// Pure: no window, no OS query. That is the point — the rule this implements is the fix for
/// a bug that only shows up on Windows, and a pure function can be tested on the machine
/// that does not have it. The bug: on macOS the menu bar is at the top, so "below the icon"
/// always fits and nobody notices. On Windows the taskbar is at the bottom, the icon's bottom
/// edge *is* the bottom of the screen, and a panel placed below it lands entirely off-screen —
/// measured at y=1446 on a 1440-tall display, i.e. invisible.
///
/// Anchoring by **available space** rather than by platform keeps the taskbar's edge (and a
/// left/right/top taskbar) from mattering.
pub fn panel_origin(icon: PixelRect, work: PixelRect, panel: (i32, i32), gap: i32) -> (i32, i32) {
    let (panel_w, panel_h) = panel;

    // Centre on the icon, then pull it back inside the work area — an icon near the right
    // edge would otherwise push half the panel past it.
    let min_x = work.x + gap;
    let max_x = (work.right() - panel_w - gap).max(min_x);
    let x = (icon.x + icon.width / 2 - panel_w / 2).clamp(min_x, max_x);

    // Below when it fits, otherwise above. Both branches are reachable in real life: the
    // first is the macOS menu bar, the second is the Windows taskbar.
    let below = icon.bottom() + gap;
    // Going above sits flush against the work area rather than hugging the icon, because the
    // icon lives *inside* the taskbar and the taskbar is *outside* the work area. Anchoring
    // to the icon would let the panel overlap the taskbar by the icon's own height.
    let above = (icon.y - gap).min(work.bottom()) - panel_h;
    let max_y = (work.bottom() - panel_h).max(work.y);

    let y = if below <= max_y {
        below
    } else if above >= work.y {
        above
    } else {
        // Taller than the space on either side of the icon. Clamp it into the work area so
        // it is at least reachable.
        max_y
    };

    (x, y)
}

fn position_under(window: &WebviewWindow, rect: Rect) {
    let (Position::Physical(icon), Size::Physical(size)) = (rect.position, rect.size) else {
        return;
    };
    let icon = PixelRect {
        x: icon.x,
        y: icon.y,
        width: size.width as i32,
        height: size.height as i32,
    };

    let scale = window.scale_factor().unwrap_or(1.0);
    let panel = ((WIDTH * scale) as i32, (HEIGHT * scale) as i32);
    let gap = (GAP * scale) as i32;

    // Ask which monitor the *icon* is on, rather than which one the window is on. The popover
    // is still hidden the first time it is positioned, so `current_monitor()` would be
    // guessing — and on a multi-monitor desk it would guess wrong and anchor the panel to the
    // wrong screen's work area.
    let centre = (
        icon.x as f64 + icon.width as f64 / 2.0,
        icon.y as f64 + icon.height as f64 / 2.0,
    );
    let Ok(Some(monitor)) = window.monitor_from_point(centre.0, centre.1) else {
        // No monitor could be resolved. Leaving the window where it is beats inventing a
        // position, and the icon is still there either way.
        eprintln!("{APP_NAME}: could not resolve the monitor under the tray icon");
        return;
    };

    // The *work area*, not the full bounds: on Windows the difference is the taskbar, and
    // using the full bounds would let the panel sit underneath it.
    let area = monitor.work_area();
    let work = PixelRect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width as i32,
        height: area.size.height as i32,
    };

    let (x, y) = panel_origin(icon, work, panel, gap);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

#[cfg(test)]
mod geometry_tests {
    // Aliased for readability in the fixtures below. The type itself is `PixelRect` because
    // `Rect` is already taken by tauri in the parent module.
    use super::{panel_origin, PixelRect as Rect};

    /// The panel size measured on Windows at 105% scaling.
    const PANEL: (i32, i32) = (336, 479);
    const GAP: i32 = 6;

    /// The display this was debugged on, to scale: 2560x1440 with a 48px taskbar, so the work
    /// area is 2560x1392 and the tray icon's bottom edge is the bottom of the screen.
    fn windows_taskbar() -> (Rect, Rect) {
        let icon = Rect { x: 2230, y: 1416, width: 24, height: 24 };
        let work = Rect { x: 0, y: 0, width: 2560, height: 1392 };
        (icon, work)
    }

    /// A macOS menu bar: at the top, icon near y=0, work area pushed down by the bar and
    /// pulled up by the Dock.
    fn macos_menu_bar() -> (Rect, Rect) {
        let icon = Rect { x: 1400, y: 0, width: 22, height: 22 };
        let work = Rect { x: 0, y: 25, width: 1512, height: 900 };
        (icon, work)
    }

    /// The bug this whole function exists for. Measured before the fix: the panel was placed
    /// at y=1446 on a 1440-tall display — six pixels below the bottom edge, entirely
    /// invisible, and the app's only UI was therefore unreachable.
    #[test]
    fn a_bottom_taskbar_puts_the_panel_above_the_icon() {
        let (icon, work) = windows_taskbar();
        let (_, y) = panel_origin(icon, work, PANEL, GAP);

        assert!(
            y + PANEL.1 <= work.bottom(),
            "panel runs past the bottom of the work area: y={y} + {} > {}",
            PANEL.1,
            work.bottom()
        );
        assert!(
            y <= icon.y,
            "panel should sit above the icon, but y={y} > icon.y={}",
            icon.y
        );
        assert!(y >= work.y, "panel should not start above the work area");
    }

    /// The reason the rule is "fit into the space" rather than "always go above": macOS puts
    /// the menu bar at the top, where below has always been correct. A fix for Windows must
    /// not move the panel on macOS.
    #[test]
    fn a_top_menu_bar_still_puts_the_panel_below_the_icon() {
        let (icon, work) = macos_menu_bar();
        let (_, y) = panel_origin(icon, work, PANEL, GAP);

        assert_eq!(
            y,
            icon.bottom() + GAP,
            "macOS placement must be unchanged by the Windows fix"
        );
    }

    /// The panel must never cover the taskbar, even though the icon it hangs off lives
    /// inside that taskbar.
    #[test]
    fn the_panel_never_covers_a_bottom_taskbar() {
        let (icon, work) = windows_taskbar();
        let (_, y) = panel_origin(icon, work, PANEL, GAP);
        assert_eq!(
            y + PANEL.1,
            work.bottom(),
            "expected the panel to sit flush against the top of the taskbar"
        );
    }

    /// An icon near the right edge would otherwise push half the panel off screen.
    #[test]
    fn the_panel_is_kept_inside_the_work_area_horizontally() {
        let work = Rect { x: 0, y: 0, width: 2560, height: 1392 };

        let far_right = Rect { x: 2540, y: 1416, width: 20, height: 24 };
        let (x, _) = panel_origin(far_right, work, PANEL, GAP);
        assert!(x + PANEL.0 <= work.right(), "panel runs off the right edge: {x}");

        let far_left = Rect { x: 0, y: 1416, width: 20, height: 24 };
        let (x, _) = panel_origin(far_left, work, PANEL, GAP);
        assert_eq!(x, work.x + GAP, "panel should be pulled in to the left margin");
    }

    /// A second monitor to the right has its own work area with a non-zero origin. Anchoring
    /// against the wrong screen's origin is exactly what `current_monitor()` used to risk.
    #[test]
    fn a_second_monitor_is_positioned_against_its_own_work_area() {
        let work = Rect { x: 2560, y: 0, width: 1920, height: 1152 };
        let icon = Rect { x: 2900, y: 1176, width: 24, height: 24 };
        let (x, y) = panel_origin(icon, work, PANEL, GAP);

        assert!(x >= work.x, "panel drifted onto the wrong monitor: x={x}");
        assert_eq!(y + PANEL.1, work.bottom());
    }

    /// The degenerate case: a work area shorter than the panel. It must still land somewhere
    /// inside the work area rather than off the top of the screen.
    #[test]
    fn a_panel_taller_than_the_work_area_stays_reachable() {
        let work = Rect { x: 0, y: 0, width: 800, height: 300 };
        let icon = Rect { x: 400, y: 320, width: 24, height: 24 };
        let (_, y) = panel_origin(icon, work, PANEL, GAP);

        assert_eq!(y, work.y, "expected the panel to be pinned to the top of the work area");
    }
}
