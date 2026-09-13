//! The scheduling engine.
//!
//! Everything here is a pure function of an injected `now`, so the whole engine is
//! testable without a UI, without a clock, and without waiting for 18:00 to arrive.
//!
//! # Algorithm
//!
//! Rather than asking "is `now` inside a peak window?" with ad-hoc comparisons, the engine
//! materialises the **set of concrete peak intervals** (absolute UTC start/end pairs) for a
//! date range, then answers questions by interval containment. Wrapping windows, weekend
//! gaps, month/year boundaries and leap days all fall out of that one representation.
//! A week of DeepSeek's schedule is 10 intervals, so the cost is irrelevant.

use crate::locale::Locale;
use crate::model::{ScheduleConfig, ScheduleError, WeeklyRule};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Serialize;

/// Which price tier applies. Only two states exist; "we don't know" is modelled by the app
/// as the *absence* of a computed state (see the tray's `Unknown`), not as a third variant —
/// the engine cannot fail to know, it can only fail to be asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PriceState {
    OffPeak,
    Peak,
}

impl PriceState {
    /// `Peak` / `Off-Peak`, or DeepSeek's own Chinese terms **高峰 / 空闲**.
    ///
    /// Not a literal translation on purpose — the official Chinese pricing page heads the
    /// two tiers 高峰时段 and 空闲时段, and that is where the user met the concept.
    pub fn label(&self, locale: Locale) -> &'static str {
        match self {
            PriceState::OffPeak => locale.pick("空闲", "Off-Peak"),
            PriceState::Peak => locale.pick("高峰", "Peak"),
        }
    }

    pub fn opposite(&self) -> Self {
        match self {
            PriceState::OffPeak => PriceState::Peak,
            PriceState::Peak => PriceState::OffPeak,
        }
    }
}

/// The next moment the tier changes, and what it changes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub at_utc: DateTime<Utc>,
    pub state: PriceState,
}

/// What the tray needs to render one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSnapshot {
    pub state: PriceState,
    pub next_change: Option<Transition>,
    /// Minutes from the queried instant until `next_change`. Clamped at 0.
    pub minutes_until_change: Option<i64>,
}

/// One contiguous stretch of a local day, for the popover's schedule list.
/// Minutes are from local midnight; `end_minute` may be 1440 (the end of the day).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySegment {
    pub start_minute: u16,
    pub end_minute: u16,
    pub state: PriceState,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    days: Vec<crate::model::Weekday>,
    /// Half-open `[start, end)` in reference-zone wall time. `end < start` wraps midnight.
    windows: Vec<(NaiveTime, NaiveTime)>,
}

/// A provider's schedule, validated and ready to query.
#[derive(Debug, Clone)]
pub struct CompiledSchedule {
    offset_minutes: i32,
    reference_label: String,
    rules: Vec<CompiledRule>,
}

impl CompiledSchedule {
    pub fn compile(cfg: &ScheduleConfig) -> Result<Self, ScheduleError> {
        if cfg.weekly.is_empty() {
            return Err(ScheduleError::NoRules);
        }
        let mut rules = Vec::with_capacity(cfg.weekly.len());
        for WeeklyRule { days, windows } in &cfg.weekly {
            if windows.is_empty() {
                return Err(ScheduleError::EmptyWindows);
            }
            let mut parsed = Vec::with_capacity(windows.len());
            for (start, end) in windows {
                let s = parse_time(start)?;
                let e = parse_time(end)?;
                if s == e {
                    return Err(ScheduleError::ZeroLengthWindow {
                        start: start.clone(),
                        end: end.clone(),
                    });
                }
                parsed.push((s, e));
            }
            rules.push(CompiledRule {
                days: days.clone(),
                windows: parsed,
            });
        }
        Ok(CompiledSchedule {
            offset_minutes: cfg.reference_utc_offset_minutes,
            reference_label: cfg
                .reference_label
                .clone()
                .unwrap_or_else(|| render_offset(cfg.reference_utc_offset_minutes)),
            rules,
        })
    }

    pub fn reference_label(&self) -> &str {
        &self.reference_label
    }

    pub fn offset_minutes(&self) -> i32 {
        self.offset_minutes
    }

    /// The rules exactly as declared: the days each rule applies to, and its windows as
    /// `("HH:MM", "HH:MM")` pairs.
    ///
    /// Deliberately returned **unformatted**. Turning these into prose for a given language
    /// is the view layer's job — keeping the engine ignorant of locale, and keeping every
    /// user-facing string in one place. This is the *published* form in the provider's own
    /// reference zone; the popover's localized day view comes from [`Self::day_segments`].
    pub fn reference_rules(&self) -> Vec<(Vec<crate::model::Weekday>, Vec<(String, String)>)> {
        self.rules
            .iter()
            .map(|rule| {
                let windows = rule
                    .windows
                    .iter()
                    .map(|(start, end)| {
                        (
                            start.format("%H:%M").to_string(),
                            end.format("%H:%M").to_string(),
                        )
                    })
                    .collect();
                (rule.days.clone(), windows)
            })
            .collect()
    }

    /// Wall-clock time in the reference zone.
    fn to_reference(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        (utc + Duration::minutes(self.offset_minutes as i64)).naive_utc()
    }

    fn from_reference(&self, naive: NaiveDateTime) -> DateTime<Utc> {
        Utc.from_utc_datetime(&(naive - Duration::minutes(self.offset_minutes as i64)))
    }

    /// Peak intervals on one reference-zone calendar date, as reference-zone wall times.
    fn windows_on_date(&self, date: NaiveDate) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        let weekday = crate::model::Weekday::from_chrono(date.weekday());
        let mut out = Vec::new();
        for rule in &self.rules {
            if !rule.days.contains(&weekday) {
                continue;
            }
            for (start, end) in &rule.windows {
                let s = date.and_time(*start);
                let mut e = date.and_time(*end);
                if e < s {
                    // Wraps past midnight. The tail belongs to *this* date's rule, which is
                    // why `state_at` also consults the previous day.
                    e += Duration::days(1);
                }
                out.push((s, e));
            }
        }
        out
    }

    /// Concrete peak intervals as absolute UTC instants, for `days` reference dates
    /// starting at `from_date`. Sorted by start.
    pub fn peak_windows_utc(
        &self,
        from_date: NaiveDate,
        days: i64,
    ) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        let mut out = Vec::new();
        for i in 0..days.max(0) {
            let date = from_date + Duration::days(i);
            for (s, e) in self.windows_on_date(date) {
                out.push((self.from_reference(s), self.from_reference(e)));
            }
        }
        out.sort();
        out
    }

    /// Which tier applies at `now`.
    ///
    /// Checks yesterday as well as today so a window wrapping past midnight is still
    /// recognised during its tail.
    pub fn state_at(&self, now: DateTime<Utc>) -> PriceState {
        let reference_now = self.to_reference(now);
        let date = reference_now.date();
        for d in [date - Duration::days(1), date] {
            for (s, e) in self.windows_on_date(d) {
                // Half-open [start, end): exactly 04:00:00 is already off-peak.
                if s <= reference_now && reference_now < e {
                    return PriceState::Peak;
                }
            }
        }
        PriceState::OffPeak
    }

    /// The next instant the tier changes, searching up to ~3 weeks ahead.
    ///
    /// Returns `None` only if the schedule has no boundaries in that horizon, which for any
    /// rule with at least one window cannot happen.
    pub fn next_transition(&self, now: DateTime<Utc>) -> Option<Transition> {
        let scan_from = self.to_reference(now).date() - Duration::days(2);
        let windows = self.peak_windows_utc(scan_from, 21);

        let mut best: Option<Transition> = None;
        for (start, end) in windows {
            for (at_utc, state) in [(start, PriceState::Peak), (end, PriceState::OffPeak)] {
                if at_utc > now && best.map_or(true, |b| at_utc < b.at_utc) {
                    best = Some(Transition { at_utc, state });
                }
            }
        }
        best
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> ScheduleSnapshot {
        let next = self.next_transition(now);
        ScheduleSnapshot {
            state: self.state_at(now),
            next_change: next,
            minutes_until_change: next.map(|t| (t.at_utc - now).num_minutes().max(0)),
        }
    }

    /// The given local calendar day, expressed as alternating off-peak/peak segments
    /// covering exactly `[00:00, 24:00)`.
    ///
    /// `display_offset_minutes` is the viewer's UTC offset. V0.1 takes it as fixed for the
    /// whole day, which is exact everywhere except on a DST transition day in a zone that
    /// observes DST — an acceptable V0.1 limitation (DeepSeek's own windows are DST-free).
    pub fn day_segments(&self, date: NaiveDate, display_offset_minutes: i32) -> Vec<DaySegment> {
        let midnight = date
            .and_hms_opt(0, 0, 0)
            .expect("midnight is always a valid time");
        let day_start = Utc.from_utc_datetime(
            &(midnight - Duration::minutes(display_offset_minutes as i64)),
        );
        let day_end = day_start + Duration::days(1);

        // Scan wide enough that a window wrapping in from two days earlier is still seen.
        let scan_from = self.to_reference(day_start - Duration::days(2)).date();
        let mut clipped: Vec<(i64, i64)> = Vec::new();
        for (s, e) in self.peak_windows_utc(scan_from, 6) {
            let cs = s.max(day_start);
            let ce = e.min(day_end);
            if cs >= ce {
                continue;
            }
            // Minutes since local midnight, which is what the popover renders.
            clipped.push((
                (cs - day_start).num_minutes().clamp(0, 1440),
                (ce - day_start).num_minutes().clamp(0, 1440),
            ));
        }
        clipped.sort();

        let mut merged: Vec<(i64, i64)> = Vec::new();
        for (a, b) in clipped {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }

        let mut segments = Vec::new();
        let mut cursor = 0i64;
        for (a, b) in merged {
            if a > cursor {
                segments.push(DaySegment {
                    start_minute: cursor as u16,
                    end_minute: a as u16,
                    state: PriceState::OffPeak,
                });
            }
            segments.push(DaySegment {
                start_minute: a as u16,
                end_minute: b as u16,
                state: PriceState::Peak,
            });
            cursor = b;
        }
        if cursor < 1440 {
            segments.push(DaySegment {
                start_minute: cursor as u16,
                end_minute: 1440,
                state: PriceState::OffPeak,
            });
        }
        segments
    }
}

/// Parse a window time, enforcing exactly the grammar the config documents.
///
/// The shape check is not redundant with chrono: `%H:%M` happily accepts one-digit fields, so
/// `"01:0"` would silently become 01:00. This config decides whether the app tells you to
/// wait or to go, so an ambiguous value should be reported rather than guessed at — and the
/// rejected set then matches what the config comment promises.
fn parse_time(raw: &str) -> Result<NaiveTime, ScheduleError> {
    let parts: Vec<&str> = raw.split(':').collect();
    let well_formed = matches!(parts.len(), 2 | 3)
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()));

    if !well_formed {
        return Err(ScheduleError::BadTime {
            raw: raw.to_string(),
            why: "expected exactly HH:MM or HH:MM:SS".to_string(),
        });
    }

    let format = if parts.len() == 2 { "%H:%M" } else { "%H:%M:%S" };
    NaiveTime::parse_from_str(raw, format).map_err(|e| ScheduleError::BadTime {
        raw: raw.to_string(),
        why: e.to_string(),
    })
}

fn render_offset(minutes: i32) -> String {
    if minutes == 0 {
        return "UTC".to_string();
    }
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.abs();
    format!("UTC{}{}:{:02}", sign, abs / 60, abs % 60)
}

/// A source of "now". Injected so tests can pin time and so `--fake-now` works in dev.
pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        self.0
    }
}

/// Environment variable that pins "now" for development and demos.
pub const FAKE_NOW_ENV: &str = "DEEPSEEKBUDGET_FAKE_NOW";

/// Parse a pinned instant. Blank or whitespace means "no pin" (`Ok(None)`).
///
/// Split out from [`clock_from_env`] so it can be tested without mutating process-global
/// environment state.
pub fn parse_fake_now(raw: &str) -> Result<Option<FixedClock>, ScheduleError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed =
        DateTime::parse_from_rfc3339(trimmed).map_err(|e| ScheduleError::BadTime {
            raw: format!("{FAKE_NOW_ENV}={raw}"),
            why: format!("expected RFC 3339, e.g. 2026-09-14T01:00:00Z ({e})"),
        })?;
    Ok(Some(FixedClock(parsed.with_timezone(&Utc))))
}

/// Resolve the clock to use, honouring [`FAKE_NOW_ENV`] if it parses as RFC 3339.
///
/// This exists because the alternative — waiting until 09:00 Beijing to check that the icon
/// turns orange — makes the peak/off-peak boundary effectively untestable by hand.
pub fn clock_from_env() -> Result<Box<dyn Clock>, ScheduleError> {
    match std::env::var(FAKE_NOW_ENV) {
        Ok(raw) => Ok(match parse_fake_now(&raw)? {
            Some(fixed) => Box::new(fixed),
            None => Box::new(SystemClock),
        }),
        Err(_) => Ok(Box::new(SystemClock)),
    }
}
