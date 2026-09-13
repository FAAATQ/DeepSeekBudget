//! The background loop that keeps the tray in sync.
//!
//! Sleeps **until the next boundary** rather than polling on a fixed interval, so the icon
//! changes exactly when the tier does rather than up to a minute late. But it never sleeps
//! longer than [`MAX_SLEEP`]: a laptop suspended across a boundary would otherwise wake with
//! a stale icon and a timer that over-runs by hours. The cap costs one trivial recompute per
//! minute, and the tray itself is only touched when the rendered result actually changes.

use deepseekbudget_schedule::StateView;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::core::{AppCore, APP_NAME};
use crate::tray;

/// Event the popover listens on to re-render itself.
pub const STATE_CHANGED: &str = "state-changed";

/// Ceiling on a single sleep. See the module comment.
const MAX_SLEEP: Duration = Duration::from_secs(60);

/// Floor on a single sleep, so a boundary that has just passed cannot spin the loop.
const MIN_SLEEP: Duration = Duration::from_secs(1);

/// How far past the boundary to aim.
///
/// `in_minutes` is whole minutes, rounded down, so sleeping exactly that long can land a
/// fraction of a second *before* the boundary and re-render the outgoing tier.
const BOUNDARY_OVERSHOOT: Duration = Duration::from_secs(1);

/// How long to wait after a panic in the refresh body before trying again.
const PANIC_BACKOFF: Duration = Duration::from_secs(30);

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        let sleep_for = match catch_unwind(AssertUnwindSafe(|| refresh(&app))) {
            Ok(duration) => duration,
            Err(_) => {
                // Unwinding is enabled on purpose (see the workspace release profile) so that
                // a panic here is catchable and costs one refresh instead of the whole tray.
                eprintln!("{APP_NAME}: refresh panicked; backing off");
                PANIC_BACKOFF
            }
        };
        std::thread::sleep(sleep_for);
    });
}

/// Recompute, update the tray if anything changed, notify the popover, and report how long
/// to sleep before the next refresh.
pub fn refresh(app: &AppHandle) -> Duration {
    let state = app.state::<Mutex<AppCore>>();
    let (changed, view) = {
        let mut core = match state.lock() {
            Ok(core) => core,
            Err(poisoned) => poisoned.into_inner(),
        };
        let view = core.view();
        let changed = core.update_rendered(&view);
        (changed, view)
    };

    if let Some(rendered) = changed {
        if let Some(tray) = app.tray_by_id(tray::TRAY_ID) {
            tray::apply_icon(&tray, rendered.state);
            let _ = tray.set_tooltip(Some(rendered.tooltip));
        }
        // Emitted only on change: the popover re-renders on this event, and waking it every
        // minute to redraw identical content would be pure waste.
        let _ = app.emit(STATE_CHANGED, &view);
    }

    sleep_for(&view)
}

fn sleep_for(view: &StateView) -> Duration {
    let Some(next) = view.next_change.as_ref() else {
        return MAX_SLEEP;
    };

    let until_boundary = Duration::from_secs(next.in_minutes.max(0) as u64 * 60);

    // Clamp *then* overshoot. Clamping afterwards — which is what this used to do — swallows
    // the overshoot for any boundary less than a minute away, which is every boundary the
    // overshoot exists to protect. The result was a constant that never did anything.
    //
    // `in_minutes == 0` means "under a minute to go", so this polls every couple of seconds
    // through the final minute and then lands 1s past the boundary. That is the intended
    // adaptive behaviour, not a busy loop: each wake is a pure recompute, and the tray is
    // only touched when the rendered result actually changes.
    until_boundary.clamp(MIN_SLEEP, MAX_SLEEP) + BOUNDARY_OVERSHOOT
}

#[cfg(test)]
mod tests {
    use super::*;
    use deepseekbudget_schedule::{build_view, ViewInput};

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    fn view_at(now: chrono::DateTime<chrono::Utc>) -> StateView {
        let config = deepseekbudget_schedule::load_bundled().unwrap();
        let schedule = deepseekbudget_schedule::bundled_schedule().unwrap();
        build_view(ViewInput {
            config: &config,
            schedule: Some(&schedule),
            error: None,
            now,
            display_offset_minutes: 480,
            display_zone_label: "Asia/Shanghai".to_string(),
            currency: "CNY".to_string(),
            locale: deepseekbudget_schedule::Locale::Zh,
            provenance: deepseekbudget_schedule::Provenance::BuiltIn,
        })
    }

    /// A boundary far in the future must not stop refreshes: the cap is what lets a laptop
    /// suspended across a boundary correct itself within a minute.
    #[test]
    fn a_distant_boundary_sleeps_at_the_cap() {
        // Monday 09:30 Beijing — 150 minutes until the 12:00 change.
        let view = view_at(utc(2026, 9, 14, 1, 30, 0));
        let sleep = sleep_for(&view);

        assert!(
            sleep >= MAX_SLEEP && sleep <= MAX_SLEEP + BOUNDARY_OVERSHOOT,
            "expected to sit at the cap, got {sleep:?}"
        );
    }

    /// The overshoot must survive clamping. It used to be applied *before* the cap, which
    /// meant it was silently discarded for every boundary less than a minute away — i.e.
    /// exactly the cases it exists to protect.
    #[test]
    fn the_overshoot_survives_the_cap() {
        let view = view_at(utc(2026, 9, 14, 3, 59, 0)); // one minute before the change
        assert_eq!(sleep_for(&view), MAX_SLEEP + BOUNDARY_OVERSHOOT);
        assert!(
            sleep_for(&view) > MAX_SLEEP,
            "clamping must not swallow the overshoot"
        );
    }

    /// Under a minute to go, `in_minutes` is 0. The loop should poll finely through the
    /// final minute — a pure recompute each time, and the tray is only touched on a change.
    #[test]
    fn the_final_minute_polls_without_spinning() {
        let view = view_at(utc(2026, 9, 14, 3, 59, 30)); // 30 seconds to go
        assert_eq!(sleep_for(&view), MIN_SLEEP + BOUNDARY_OVERSHOOT);
    }

    /// Whatever the state — mid-peak, exactly on a boundary, a second before one, or a
    /// weekend with the next change days away — the sleep is positive and bounded.
    #[test]
    fn sleep_is_always_positive_and_bounded() {
        for now in [
            utc(2026, 9, 14, 1, 30, 0),  // mid-peak
            utc(2026, 9, 14, 4, 0, 0),   // exactly on a boundary
            utc(2026, 9, 14, 3, 59, 59), // one second before one
            utc(2026, 9, 12, 2, 0, 0),   // weekend: next change is ~2 days away
        ] {
            let sleep = sleep_for(&view_at(now));
            assert!(
                sleep >= MIN_SLEEP && sleep <= MAX_SLEEP + BOUNDARY_OVERSHOOT,
                "{now} produced {sleep:?}"
            );
        }
    }
}
