//! DeepSeek Budget's scheduling engine.
//!
//! Answers two questions and nothing else:
//!
//! 1. Is the provider's API in its peak or off-peak pricing tier **right now**?
//! 2. When does that next change, and to what?
//!
//! No network, no clock of its own, no UI. `now` is always passed in, which is what makes
//! the peak/off-peak boundary testable instead of something you verify by waiting.
//!
//! ```
//! let schedule = deepseekbudget_schedule::bundled_schedule().unwrap();
//! let now = chrono::DateTime::parse_from_rfc3339("2026-09-14T02:00:00Z")
//!     .unwrap()
//!     .with_timezone(&chrono::Utc);
//! assert_eq!(
//!     schedule.state_at(now),
//!     deepseekbudget_schedule::PriceState::Peak,
//! );
//! ```

pub mod display;
pub mod engine;
pub mod locale;
pub mod model;
pub mod validate;
pub mod view;

#[cfg(test)]
mod tests;

pub use engine::{
    Clock, CompiledSchedule, DaySegment, FAKE_NOW_ENV, FixedClock, PriceState, ScheduleSnapshot,
    SystemClock, Transition, clock_from_env, parse_fake_now,
};
pub use locale::Locale;
pub use model::{
    ModelConfig, ModelPrices, ProviderConfig, ScheduleConfig, ScheduleError, Tier, TieredPrice,
    Weekday, WeeklyRule,
};
pub use validate::{ConfigRejection, MAX_PRICE_RATIO, accept};
pub use view::{
    ModelView, NextChangeView, Provenance, SegmentView, StateView, TierRow, ViewInput, build_view,
    currency_symbol, format_money, format_price, rule_label, tooltip_lines,
};

/// The bundled DeepSeek pricing and schedule definition, transcribed from
/// <https://api-docs.deepseek.com/quick_start/pricing> on the date in `verifiedAt`.
///
/// Baked into the binary on purpose. This is the floor the app can always fall back to: it is
/// present offline, it cannot be corrupted after the fact, and the figures in it are exactly
/// the ones this build was reviewed and tested against.
///
/// It is no longer the *only* source — the app can fetch a newer copy at the user's request and
/// install it over this one (see [`validate::accept`]). But a synced copy is an overlay, never a
/// replacement: everything that reads a config can still assume this one parses.
pub const BUNDLED_DEEPSEEK_JSON: &str = include_str!("../config/providers/deepseek.json");

pub fn load_bundled() -> Result<ProviderConfig, ScheduleError> {
    let cfg: ProviderConfig = serde_json::from_str(BUNDLED_DEEPSEEK_JSON)
        .map_err(|e| ScheduleError::Json(e.to_string()))?;
    Ok(cfg)
}

/// The bundled provider's schedule, compiled and ready to query.
pub fn bundled_schedule() -> Result<CompiledSchedule, ScheduleError> {
    CompiledSchedule::compile(&load_bundled()?.schedule)
}
