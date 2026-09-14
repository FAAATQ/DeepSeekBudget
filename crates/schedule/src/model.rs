//! Serde types for the bundled provider config, plus the error type.
//!
//! Config shapes are deliberately dumb here — nothing is parsed or validated at
//! deserialize time beyond JSON structure. All semantic validation happens in
//! [`crate::engine::CompiledSchedule::compile`], so a malformed config surfaces as a single
//! well-described error (which the UI shows as the `Unknown` state) rather than as an
//! arbitrary serde message.

use crate::locale::Locale;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A day of the week, as written in config (`"Mon"`, `"Tue"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn from_chrono(w: chrono::Weekday) -> Self {
        use chrono::Weekday as C;
        match w {
            C::Mon => Weekday::Mon,
            C::Tue => Weekday::Tue,
            C::Wed => Weekday::Wed,
            C::Thu => Weekday::Thu,
            C::Fri => Weekday::Fri,
            C::Sat => Weekday::Sat,
            C::Sun => Weekday::Sun,
        }
    }

    /// `Mon` / `周一`.
    pub fn short(&self, locale: Locale) -> &'static str {
        let (zh, en) = match self {
            Weekday::Mon => ("周一", "Mon"),
            Weekday::Tue => ("周二", "Tue"),
            Weekday::Wed => ("周三", "Wed"),
            Weekday::Thu => ("周四", "Thu"),
            Weekday::Fri => ("周五", "Fri"),
            Weekday::Sat => ("周六", "Sat"),
            Weekday::Sun => ("周日", "Sun"),
        };
        locale.pick(zh, en)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub provider: String,
    pub display_name: String,
    pub source_url: String,
    /// Date the figures below were transcribed from `source_url`, as `YYYY-MM-DD`.
    pub verified_at: String,
    pub default_currency: String,
    pub schedule: ScheduleConfig,
    pub models: Vec<ModelConfig>,
    /// Free-form notes for the About panel, **keyed by locale tag** so the provider's own
    /// wording appears in the reader's language — mirroring how `prices` is keyed by
    /// currency. A locale with no entry falls back to English.
    #[serde(default)]
    pub notes: BTreeMap<String, Vec<String>>,
}

impl ProviderConfig {
    /// Notes for a locale, falling back to English when that locale has none.
    pub fn notes_for(&self, locale: Locale) -> Vec<String> {
        self.notes
            .get(locale.tag())
            .or_else(|| self.notes.get(Locale::En.tag()))
            .cloned()
            .unwrap_or_default()
    }
}

/// When peak pricing applies.
///
/// The schedule is defined in a **reference zone**, not in the user's local zone. This
/// matters: day-of-week must be judged in the reference zone too. For a UTC-5 user,
/// Monday 01:00 UTC is *Sunday* 20:00 local — judging weekdays locally would shift the
/// whole week by a day.
///
/// V0.1 uses a fixed UTC offset rather than a timezone database. That is exactly correct
/// for DeepSeek (its windows are quoted in UTC, which has no DST) and it avoids shipping
/// ~1MB of tzdata. A provider whose reference zone observes DST would need `chrono-tz`;
/// the type is shaped so that swapping in a real zone later is a local change.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleConfig {
    pub reference_utc_offset_minutes: i32,
    /// Human label for the reference zone, e.g. `"UTC"`. Falls back to a rendered offset.
    #[serde(default)]
    pub reference_label: Option<String>,
    pub weekly: Vec<WeeklyRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeeklyRule {
    /// Days this rule's windows apply to, judged in the reference zone.
    pub days: Vec<Weekday>,
    /// Each entry is `["HH:MM", "HH:MM"]`, half-open `[start, end)` in the reference zone.
    /// An `end` strictly earlier than `start` wraps past midnight into the next day.
    /// `end == start` is rejected as a config error.
    pub windows: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    /// The model id to pass to the API, e.g. `"deepseek-flash"`.
    pub id: String,
    /// Short label for the UI, e.g. `"V4.1 Flash"`.
    pub label: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Keyed by currency code (`"CNY"`, `"USD"`). DeepSeek publishes both directly, so
    /// there is no FX conversion anywhere in this app.
    pub prices: BTreeMap<String, ModelPrices>,
}

impl ModelConfig {
    pub fn price_in(&self, currency: &str) -> Option<&ModelPrices> {
        self.prices.get(currency)
    }
}

/// Prices per 1M tokens.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrices {
    pub input_cache_hit: TieredPrice,
    pub input_cache_miss: TieredPrice,
    pub output: TieredPrice,
}

impl ModelPrices {
    /// Every published rate, paired with the name it goes by in config JSON.
    ///
    /// This exists so that validation can walk a config **exhaustively** rather than naming
    /// fields one at a time. A field added to this struct without being added here would
    /// silently skip its own checks, so a test in `validate` fails if the two ever drift.
    /// The names are the JSON ones on purpose: they are what a maintainer editing the config
    /// typed, and what an error message should quote back.
    pub fn fields(&self) -> [(&'static str, TieredPrice); 3] {
        [
            ("inputCacheHit", self.input_cache_hit),
            ("inputCacheMiss", self.input_cache_miss),
            ("output", self.output),
        ]
    }

    /// Look a rate up by its config field name.
    pub fn field(&self, name: &str) -> Option<TieredPrice> {
        self.fields()
            .into_iter()
            .find(|(field, _)| *field == name)
            .map(|(_, tiered)| tiered)
    }
}

/// Which of the two price tiers. A value rather than a bare `&str` so the two can be iterated
/// without hard-coding their names at every use site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    OffPeak,
    Peak,
}

impl Tier {
    pub const ALL: [Tier; 2] = [Tier::OffPeak, Tier::Peak];

    /// The name this tier goes by in config JSON.
    pub fn field_name(self) -> &'static str {
        match self {
            Tier::OffPeak => "offPeak",
            Tier::Peak => "peak",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TieredPrice {
    pub off_peak: f64,
    pub peak: f64,
}

impl TieredPrice {
    /// The rate for one tier, so callers can loop over [`Tier::ALL`] instead of naming the two
    /// fields separately in each of them.
    pub fn for_tier(self, tier: Tier) -> f64 {
        match tier {
            Tier::OffPeak => self.off_peak,
            Tier::Peak => self.peak,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// Config JSON could not be deserialized.
    Json(String),
    /// A window time was not `HH:MM` / `HH:MM:SS`.
    BadTime { raw: String, why: String },
    /// A rule declared no windows.
    EmptyWindows,
    /// A window's start and end are identical, which would mean either zero length or a
    /// full day. Almost certainly a typo, so it is rejected rather than guessed at.
    ZeroLengthWindow { start: String, end: String },
    /// The config declared no rules at all.
    NoRules,
    /// A rule listed no weekdays, so it can never match. A schedule made only of such rules is
    /// permanently off-peak *and* has no boundary to count down to, which is the same outcome
    /// `NoRules` rejects — expressed the other way round, so it is rejected the same way.
    EmptyDays,
    /// A UTC offset no real zone uses. `-12:00`..`+14:00` is the whole inhabited range;
    /// anything outside it is a typo, and the large values are worse than typos: `i32::MIN`
    /// used to overflow in the label formatter.
    AbsurdOffset { minutes: i32 },
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleError::Json(e) => write!(f, "provider config is not valid JSON: {e}"),
            ScheduleError::BadTime { raw, why } => {
                write!(f, "window time {raw:?} is not HH:MM: {why}")
            }
            ScheduleError::EmptyWindows => write!(f, "a schedule rule declared no windows"),
            ScheduleError::ZeroLengthWindow { start, end } => write!(
                f,
                "window {start}–{end} has identical start and end; \
                 use a wrapping window for a full day, or fix the typo"
            ),
            ScheduleError::NoRules => write!(f, "schedule declared no weekly rules"),
            ScheduleError::EmptyDays => {
                write!(f, "a schedule rule listed no weekdays, so it can never apply")
            }
            ScheduleError::AbsurdOffset { minutes } => write!(
                f,
                "referenceUtcOffsetMinutes is {minutes}; real zones run from -720 (-12:00) \
                 to 840 (+14:00)"
            ),
        }
    }
}

impl std::error::Error for ScheduleError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_maps_from_chrono() {
        use chrono::Weekday as C;
        assert_eq!(Weekday::from_chrono(C::Mon), Weekday::Mon);
        assert_eq!(Weekday::from_chrono(C::Sun), Weekday::Sun);
    }

    #[test]
    fn weekday_short_names_round_trip_through_json() {
        for d in [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ] {
            // The config's day names are English identifiers regardless of the UI language.
            let json = serde_json::to_string(&d).unwrap();
            assert_eq!(json, format!("\"{}\"", d.short(Locale::En)));
            let back: Weekday = serde_json::from_str(&json).unwrap();
            assert_eq!(back, d);
        }
    }

    #[test]
    fn weekday_names_are_localized_and_distinct() {
        for d in [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ] {
            let en = d.short(Locale::En);
            let zh = d.short(Locale::Zh);
            assert_ne!(en, zh, "{d:?} did not translate");
            assert!(zh.starts_with('周'), "{d:?} Chinese name should start with 周");
        }
    }

    #[test]
    fn two_element_window_deserializes_from_a_pair() {
        let rule: WeeklyRule =
            serde_json::from_str(r#"{"days":["Mon"],"windows":[["01:00","04:00"]]}"#).unwrap();
        assert_eq!(rule.windows, vec![("01:00".to_string(), "04:00".to_string())]);
    }

    #[test]
    fn price_lookup_is_by_currency_code() {
        let cfg = crate::load_bundled().unwrap();
        let flash = &cfg.models[0];
        assert_eq!(flash.price_in("CNY").unwrap().output.peak, 8.0);
        assert_eq!(flash.price_in("USD").unwrap().output.peak, 1.2);
        assert!(flash.price_in("EUR").is_none());
    }

    #[test]
    fn notes_are_selected_by_locale() {
        let cfg = crate::load_bundled().unwrap();
        let en = cfg.notes_for(Locale::En);
        let zh = cfg.notes_for(Locale::Zh);

        assert!(!en.is_empty() && !zh.is_empty());
        assert_ne!(en, zh, "the bundled provider must ship notes in both languages");
        // The Chinese set is genuinely Chinese, not the English list relabelled.
        assert!(zh.iter().any(|note| note.contains("空闲")), "{zh:?}");
    }

    #[test]
    fn notes_fall_back_to_english_and_tolerate_being_absent() {
        let mut cfg = crate::load_bundled().unwrap();
        cfg.notes.remove(Locale::Zh.tag());
        assert_eq!(cfg.notes_for(Locale::Zh), cfg.notes_for(Locale::En));

        cfg.notes.clear();
        assert!(
            cfg.notes_for(Locale::Zh).is_empty(),
            "a provider with no notes at all is not an error"
        );
    }

    #[test]
    fn errors_are_human_readable() {
        let e = ScheduleError::ZeroLengthWindow {
            start: "09:00".into(),
            end: "09:00".into(),
        };
        assert!(e.to_string().contains("identical start and end"));
    }
}
