//! Builds the exact strings the UI shows.
//!
//! Deliberately not in the UI layer: the tray tooltip and the popover are two different
//! renderers for the same facts, and putting the formatting here means they cannot disagree —
//! and that the wording, the price rounding and the countdown phrasing are all unit-tested
//! without spinning up a window.

use crate::display::{
    format_minute_of_day, humanize_duration, local_clock, local_day_and_clock, relative_day_label,
};
use crate::engine::{CompiledSchedule, PriceState};
use crate::locale::Locale;
use crate::model::{ModelConfig, ProviderConfig, Weekday};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// Currency symbol for display. Falls back to the code itself for anything unknown, so a
/// new provider's currency renders sensibly without a code change.
pub fn currency_symbol(code: &str) -> &str {
    match code {
        "CNY" => "¥",
        "USD" => "$",
        "EUR" => "€",
        "JPY" => "¥",
        "GBP" => "£",
        other => other,
    }
}

/// Format a published price. Trailing zeros are trimmed so the table stays narrow:
/// `0.02`, `1`, `4.5`, `27`.
pub fn format_price(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let rendered = format!("{value:.4}");
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Join a currency symbol to an amount. A real symbol hugs the number (`¥8`, `$8`); a
/// fallback currency *code* needs a space (`SGD 8`).
pub fn format_money(symbol: &str, amount: &str) -> String {
    if !symbol.is_empty() && symbol.chars().all(|c| c.is_ascii_alphabetic()) {
        format!("{symbol} {amount}")
    } else {
        format!("{symbol}{amount}")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRow {
    pub label: String,
    pub off_peak: String,
    pub peak: String,
    /// The figure in force right now — the one the user actually cares about.
    pub active: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub id: String,
    pub label: String,
    pub version: Option<String>,
    pub rows: Vec<TierRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    pub start_label: String,
    pub end_label: String,
    pub state: PriceState,
    pub state_label: String,
    /// Whether "now" falls in this segment.
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextChangeView {
    pub state: PriceState,
    pub state_label: String,
    /// `Today 18:00` / `Mon 09:00`.
    pub at_label: String,
    /// `3h 28m`.
    pub in_label: String,
    /// Raw minutes until the change. The UI only shows `in_label`, but the tray's tick loop
    /// needs the number to sleep exactly until the boundary rather than polling for it.
    pub in_minutes: i64,
}

/// Where the provider config in force came from.
///
/// Supplied by the app rather than derived here, for the same reason `display_zone_label` is:
/// only the app knows whether a synced copy is on disk. It is *rendered* here because producing
/// user-facing text is this module's job, and the provenance line sits next to the provider name
/// and verification date that this module already formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provenance {
    /// The copy compiled into the binary. The default, and the only possibility if the user
    /// has never synced.
    #[default]
    BuiltIn,
    /// A copy fetched from the provider's published config and applied at the user's request.
    Synced,
}

/// Everything the popover and tooltip render. One payload, two renderers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    /// False when the schedule could not be resolved; the UI then shows the Unknown icon and
    /// surfaces `error` instead of a tier. Prices are still rendered — they do not depend on
    /// the schedule being parseable.
    pub ok: bool,
    pub error: Option<String>,

    /// The locale this payload was rendered in, as a tag (`"zh"` / `"en"`). The UI uses it
    /// to set `<html lang>` and to pick its own (small) dictionary of static labels.
    pub locale: String,

    pub state: Option<PriceState>,
    pub state_label: String,
    pub now_label: String,
    pub today_label: String,
    /// Name of the zone the schedule is rendered in, e.g. `Asia/Shanghai`.
    pub display_zone_label: String,
    /// That zone as an offset, e.g. `UTC+08:00`.
    pub offset_label: String,

    pub next_change: Option<NextChangeView>,
    pub segments: Vec<SegmentView>,

    pub models: Vec<ModelView>,
    pub currency: String,
    pub currency_symbol: String,
    pub available_currencies: Vec<String>,
    pub units_label: String,

    pub provider_name: String,
    pub source_url: String,
    pub verified_at: String,
    /// `Built-in · 2026-09-12` / `已同步 · 2026-09-20` — which copy of the figures is in force,
    /// and the date they were transcribed from the provider's page.
    ///
    /// Shown wherever the prices appear, because a user reading a number off this app is
    /// entitled to know whether it came from the binary they installed or from a fetch. It is
    /// the one piece of honesty the update mechanism makes necessary.
    pub config_source_label: String,
    /// The peak windows as published, in the schedule's own reference zone.
    pub rule_label: String,
    pub reference_label: String,
    /// The provider's own notes, straight from the config, rendered as small print. Kept in
    /// the config rather than the UI so provider-specific facts stay data.
    pub notes: Vec<String>,
    /// One line under the price table spelling out what the *other* tier costs right now.
    ///
    /// The table itself only shows the tier in force, which is the right thing to read at a
    /// glance — but on its own it hides the single most motivating fact this app has, which
    /// is that waiting for off-peak halves the bill. Stating the other number turns the panel
    /// from a readout into a decision.
    pub tier_note: String,
}

pub struct ViewInput<'a> {
    pub config: &'a ProviderConfig,
    /// `None` when the schedule failed to compile.
    pub schedule: Option<&'a CompiledSchedule>,
    pub error: Option<String>,
    pub now: DateTime<Utc>,
    pub display_offset_minutes: i32,
    /// Human name for the viewer's zone. Supplied by the app rather than derived here,
    /// because only the app knows whether the user chose the system zone or an override.
    pub display_zone_label: String,
    pub currency: String,
    /// Interface language for every string this function produces.
    pub locale: Locale,
    /// Where the config came from. The app supplies this; see [`Provenance`].
    pub provenance: Provenance,
}

pub fn build_view(input: ViewInput<'_>) -> StateView {
    let ViewInput {
        config,
        schedule,
        error,
        now,
        display_offset_minutes,
        display_zone_label,
        currency,
        locale,
        provenance,
    } = input;

    let today_local = crate::display::local_date(now, display_offset_minutes);
    let state = schedule.map(|s| s.state_at(now));

    let next_change = schedule.and_then(|s| s.next_transition(now)).map(|t| {
        let in_minutes = (t.at_utc - now).num_minutes().max(0);
        NextChangeView {
            state: t.state,
            state_label: t.state.label(locale).to_string(),
            at_label: local_day_and_clock(t.at_utc, display_offset_minutes, today_local, locale),
            in_label: humanize_duration(in_minutes, locale),
            in_minutes,
        }
    });

    let now_minute = local_minute_of_day(now, display_offset_minutes);
    let segments = schedule
        .map(|s| {
            s.day_segments(today_local, display_offset_minutes)
                .into_iter()
                .map(|seg| SegmentView {
                    start_label: format_minute_of_day(seg.start_minute),
                    end_label: format_minute_of_day(seg.end_minute),
                    state_label: seg.state.label(locale).to_string(),
                    state: seg.state,
                    is_current: (seg.start_minute as i64) <= now_minute
                        && now_minute < seg.end_minute as i64,
                })
                .collect()
        })
        .unwrap_or_default();

    let models = config
        .models
        .iter()
        .map(|m| model_view(m, &currency, state, locale))
        .collect();
    let mut available_currencies: Vec<String> = config
        .models
        .iter()
        .flat_map(|m| m.prices.keys().cloned())
        .collect();
    available_currencies.sort();
    available_currencies.dedup();

    let rule_label = schedule
        .map(|s| rule_label(s, locale))
        .unwrap_or_else(|| {
            locale
                .pick("（排程不可用）", "(schedule unavailable)")
                .to_string()
        });
    let reference_label = schedule
        .map(|s| s.reference_label().to_string())
        .unwrap_or_default();
    // Computed before the struct literal: `currency` is moved into a field below.
    let tier_note = tier_note(config, &currency, state, locale);

    StateView {
        ok: error.is_none() && schedule.is_some(),
        error,
        locale: locale.tag().to_string(),
        state,
        state_label: state.map(|s| s.label(locale).to_string()).unwrap_or_default(),
        now_label: local_clock(now, display_offset_minutes),
        today_label: relative_day_label(today_local, today_local, locale),
        display_zone_label,
        offset_label: crate::display::render_offset(display_offset_minutes),
        next_change,
        segments,
        models,
        currency_symbol: currency_symbol(&currency).to_string(),
        currency,
        available_currencies,
        units_label: locale.pick("每百万 tokens", "per 1M tokens").to_string(),
        provider_name: config.display_name.clone(),
        source_url: config.source_url.clone(),
        verified_at: config.verified_at.clone(),
        config_source_label: config_source_label(config, provenance, locale),
        rule_label,
        reference_label,
        notes: config.notes_for(locale),
        tier_note,
    }
}

/// `Built-in · 2026-09-12` / `已同步 · 2026-09-20`.
///
/// The date is the provider's own `verifiedAt`, not the moment of the fetch: what a user needs
/// to judge a figure is how old the *figure* is, not how long ago their app talked to GitHub.
fn config_source_label(config: &ProviderConfig, provenance: Provenance, locale: Locale) -> String {
    let source = match provenance {
        Provenance::BuiltIn => locale.pick("内置", "Built-in"),
        Provenance::Synced => locale.pick("已同步", "Synced"),
    };
    // A placeholder config (the bundled JSON failed to parse) has no date. Trailing "· " would
    // be worse than saying less.
    let date = config.verified_at.trim();
    if date.is_empty() {
        return source.to_string();
    }
    format!("{source} · {date}")
}

/// `Peak in effect: ¥8 / 1M output (V4.1 Flash). Off-peak is ¥4 — waiting halves it.`
/// Chinese: `当前高峰价：¥8 / 百万 tokens 输出（V4.1 Flash）。空闲价为 ¥4，等一等省一半。`
///
/// Names the headline model, because the figure is meaningless without knowing which model
/// it belongs to.
fn tier_note(
    config: &ProviderConfig,
    currency: &str,
    state: Option<PriceState>,
    locale: Locale,
) -> String {
    let Some(state) = state else {
        return String::new();
    };
    let label = state.label(locale);

    // Used when the config has no models or not in this currency — still needs to say which
    // tier is in force.
    let bare = || match locale {
        Locale::Zh => format!("当前{label}价。"),
        Locale::En => format!("{label} prices in effect."),
    };

    let Some(model) = config.models.first() else {
        return bare();
    };
    let Some(prices) = model.price_in(currency) else {
        return bare();
    };

    let symbol = currency_symbol(currency);
    let money = |value: f64| format_money(symbol, &format_price(value));
    let (now, other) = match state {
        PriceState::Peak => (prices.output.peak, prices.output.off_peak),
        PriceState::OffPeak => (prices.output.off_peak, prices.output.peak),
    };
    let other_label = state.opposite().label(locale);
    let units = locale.pick("百万 tokens 输出", "1M output");

    match (locale, state) {
        (Locale::En, PriceState::Peak) => format!(
            "{label} in effect: {} / {units} ({}). {other_label} is {} — waiting halves it.",
            money(now),
            model.label,
            money(other)
        ),
        (Locale::En, PriceState::OffPeak) => format!(
            "{label} in effect: {} / {units} ({}). {other_label} is {}.",
            money(now),
            model.label,
            money(other)
        ),
        (Locale::Zh, PriceState::Peak) => format!(
            "当前{label}价：{} / {units}（{}）。{other_label}价为 {}，等一等省一半。",
            money(now),
            model.label,
            money(other)
        ),
        (Locale::Zh, PriceState::OffPeak) => format!(
            "当前{label}价：{} / {units}（{}）。{other_label}价为 {}。",
            money(now),
            model.label,
            money(other)
        ),
    }
}

fn model_view(
    model: &ModelConfig,
    currency: &str,
    state: Option<PriceState>,
    locale: Locale,
) -> ModelView {
    let rows = match model.price_in(currency) {
        Some(p) => {
            let row = |label: &str, off_peak: f64, peak: f64| TierRow {
                label: label.to_string(),
                off_peak: format_price(off_peak),
                peak: format_price(peak),
                active: format_price(match state {
                    Some(PriceState::Peak) => peak,
                    _ => off_peak,
                }),
            };
            // The three tiers, in DeepSeek's own Chinese terms (输入/输出, 缓存命中/未命中).
            vec![
                row(
                    locale.pick("输入 · 缓存命中", "input · cache hit"),
                    p.input_cache_hit.off_peak,
                    p.input_cache_hit.peak,
                ),
                row(
                    locale.pick("输入 · 缓存未命中", "input · cache miss"),
                    p.input_cache_miss.off_peak,
                    p.input_cache_miss.peak,
                ),
                row(
                    locale.pick("输出", "output"),
                    p.output.off_peak,
                    p.output.peak,
                ),
            ]
        }
        None => Vec::new(),
    };
    ModelView {
        id: model.id.clone(),
        label: model.label.clone(),
        version: model.version.clone(),
        rows,
    }
}

fn local_minute_of_day(at_utc: DateTime<Utc>, display_offset_minutes: i32) -> i64 {
    let shifted = (at_utc + Duration::minutes(display_offset_minutes as i64)).naive_utc();
    use chrono::Timelike;
    shifted.hour() as i64 * 60 + shifted.minute() as i64
}

/// `Mon–Fri 01:00–04:00, 06:00–10:00 UTC` — the published rule, in the provider's own
/// reference zone. Chinese: `周一至周五 01:00–04:00、06:00–10:00 UTC`.
///
/// The popover's localized view is the `segments` list; this line exists so the user can
/// always see the authoritative rule the app is applying.
pub fn rule_label(schedule: &CompiledSchedule, locale: Locale) -> String {
    let window_separator = locale.pick("、", ", ");
    let rule_separator = locale.pick("；", "; ");

    let parts: Vec<String> = schedule
        .reference_rules()
        .iter()
        .map(|(days, windows)| {
            let ranges: Vec<String> = windows
                .iter()
                .map(|(start, end)| format!("{start}–{end}"))
                .collect();
            format!("{} {}", render_days(days, locale), ranges.join(window_separator))
        })
        .collect();

    format!(
        "{} {}",
        parts.join(rule_separator),
        schedule.reference_label()
    )
}

/// Collapse consecutive weekdays into ranges: `[Mon..Fri]` → `Mon–Fri` / `周一至周五`.
pub fn render_days(days: &[Weekday], locale: Locale) -> String {
    if days.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<Weekday> = days.to_vec();
    sorted.sort();
    sorted.dedup();

    let mut runs: Vec<Vec<Weekday>> = Vec::new();
    for day in sorted {
        match runs.last_mut() {
            Some(run) if is_next(run[run.len() - 1], day) => run.push(day),
            _ => runs.push(vec![day]),
        }
    }

    runs.iter()
        .map(|run| {
            if run.len() == 1 {
                run[0].short(locale).to_string()
            } else {
                // The range separator is language-specific: an en dash between Chinese words
                // is a Western import. Chinese writes 周一至周五 — which is also exactly how
                // DeepSeek's own Chinese pricing page writes it.
                format!(
                    "{}{}{}",
                    run[0].short(locale),
                    locale.pick("至", "–"),
                    run[run.len() - 1].short(locale)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(locale.pick("、", ", "))
}

fn is_next(previous: Weekday, next: Weekday) -> bool {
    let index = |d: Weekday| {
        [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ]
        .iter()
        .position(|x| *x == d)
        .unwrap() as i32
    };
    index(next) == index(previous) + 1
}

/// Tooltip lines. macOS renders a real multi-line tooltip; Windows tray tooltips are
/// length-limited and multi-line support is unreliable, so it gets a compressed form and the
/// popover carries the detail.
pub fn tooltip_lines(view: &StateView, compact: bool) -> Vec<String> {
    // Derived from the view rather than taken as a parameter, so the tooltip cannot end up
    // speaking a different language from the panel it mirrors.
    let locale = Locale::from_tag(&view.locale);

    // The tooltip names one model so the price line is unambiguous; the popover lists all of
    // them. The first model in config is the provider's headline model.
    let headline = view.models.first();
    let price_line = headline.and_then(|m| {
        m.rows.last().map(|row| {
            format!(
                "{} / {}",
                format_money(&view.currency_symbol, &row.active),
                locale.pick("百万 tokens 输出", "1M output")
            )
        })
    });

    let state_line = if view.ok {
        view.state_label.clone()
    } else {
        locale.pick("未知", "Unknown").to_string()
    };

    let next_line = view.next_change.as_ref().map(|n| match (locale, compact) {
        (Locale::En, true) => format!("Next {} {}", n.state_label, n.at_label),
        (Locale::En, false) => format!("Next {}: {} ({})", n.state_label, n.at_label, n.in_label),
        (Locale::Zh, true) => format!("下次{} {}", n.state_label, n.at_label),
        (Locale::Zh, false) => format!("下次{}：{}（{}）", n.state_label, n.at_label, n.in_label),
    });

    if compact {
        let mut lines = vec![format!("DeepSeek Budget · {state_line}")];
        if let Some(line) = price_line {
            lines.push(line);
        }
        if let Some(line) = next_line {
            lines.push(line);
        }
        lines
    } else {
        let mut lines = vec!["DeepSeek Budget".to_string()];
        lines.push(match headline {
            Some(m) => format!("{} · {}", view.provider_name, m.label),
            None => view.provider_name.clone(),
        });
        lines.push(state_line);
        if let Some(line) = price_line {
            lines.push(line);
        }
        if let Some(line) = next_line {
            lines.push(line);
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    /// English by default — most assertions are about the schedule, not the wording.
    fn view_at(now: DateTime<Utc>, offset: i32, currency: &str) -> StateView {
        view_at_locale(now, offset, currency, Locale::En)
    }

    fn view_at_locale(now: DateTime<Utc>, offset: i32, currency: &str, locale: Locale) -> StateView {
        let config = crate::load_bundled().unwrap();
        let schedule = crate::bundled_schedule().unwrap();
        build_view(ViewInput {
            config: &config,
            schedule: Some(&schedule),
            error: None,
            now,
            display_offset_minutes: offset,
            display_zone_label: "Asia/Shanghai".to_string(),
            currency: currency.to_string(),
            locale,
            provenance: Provenance::BuiltIn,
        })
    }

    #[test]
    fn prices_render_without_trailing_zeros() {
        assert_eq!(format_price(0.02), "0.02");
        assert_eq!(format_price(0.003), "0.003");
        assert_eq!(format_price(1.0), "1");
        assert_eq!(format_price(4.5), "4.5");
        assert_eq!(format_price(13.5), "13.5");
        assert_eq!(format_price(27.0), "27");
        assert_eq!(format_price(0.3), "0.3");
        assert_eq!(format_price(0.0), "0");
    }

    #[test]
    fn currency_symbols_fall_back_to_the_code() {
        assert_eq!(currency_symbol("CNY"), "¥");
        assert_eq!(currency_symbol("USD"), "$");
        assert_eq!(currency_symbol("SGD"), "SGD");
    }

    #[test]
    fn money_hugs_a_symbol_but_spaces_a_code() {
        assert_eq!(format_money("¥", "8"), "¥8");
        assert_eq!(format_money("$", "0.02"), "$0.02");
        // A currency *code* fallback would otherwise read as "SGD8".
        assert_eq!(format_money("SGD", "8"), "SGD 8");
    }

    #[test]
    fn consecutive_days_collapse_into_ranges() {
        let en = Locale::En;
        let weekdays = vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ];
        assert_eq!(render_days(&weekdays, en), "Mon–Fri");
        assert_eq!(render_days(&[Weekday::Sat, Weekday::Sun], en), "Sat–Sun");
        assert_eq!(render_days(&[Weekday::Mon], en), "Mon");
        assert_eq!(render_days(&[Weekday::Mon, Weekday::Wed], en), "Mon, Wed");
        // Out-of-order and duplicated input still collapses.
        assert_eq!(
            render_days(&[Weekday::Tue, Weekday::Mon, Weekday::Mon], en),
            "Mon–Tue"
        );
        assert_eq!(render_days(&[], en), "");
    }

    #[test]
    fn consecutive_days_collapse_into_ranges_in_chinese() {
        let zh = Locale::Zh;
        let weekdays = vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ];
        assert_eq!(render_days(&weekdays, zh), "周一至周五");
        assert_eq!(render_days(&[Weekday::Sat, Weekday::Sun], zh), "周六至周日");
        assert_eq!(render_days(&[Weekday::Mon], zh), "周一");
        // Chinese lists use 、 rather than a comma.
        assert_eq!(render_days(&[Weekday::Mon, Weekday::Wed], zh), "周一、周三");
    }

    #[test]
    fn the_published_rule_is_rendered_in_the_reference_zone() {
        let schedule = crate::bundled_schedule().unwrap();
        assert_eq!(
            rule_label(&schedule, Locale::En),
            "Mon–Fri 01:00–04:00, 06:00–10:00 UTC"
        );
        assert_eq!(
            rule_label(&schedule, Locale::Zh),
            "周一至周五 01:00–04:00、06:00–10:00 UTC"
        );
    }

    #[test]
    fn view_reports_the_active_price_for_the_current_tier() {
        // Monday 02:00 UTC = 10:00 Beijing, inside the morning peak.
        let view = view_at(utc(2026, 9, 14, 2, 0), 480, "CNY");
        assert!(view.ok);
        assert_eq!(view.state, Some(PriceState::Peak));
        assert_eq!(view.state_label, "Peak");

        let flash = &view.models[0];
        assert_eq!(flash.id, "deepseek-flash");
        assert_eq!(flash.rows[2].label, "output");
        assert_eq!(flash.rows[2].off_peak, "4");
        assert_eq!(flash.rows[2].peak, "8");
        // Peak is in force, so the active figure is the peak one.
        assert_eq!(flash.rows[2].active, "8");

        let pro = &view.models[1];
        assert_eq!(pro.id, "deepseek-v4-pro");
        assert_eq!(pro.rows[2].active, "27");
    }

    #[test]
    fn off_peak_view_activates_the_off_peak_figures() {
        let view = view_at(utc(2026, 9, 14, 12, 0), 480, "CNY"); // 20:00 Beijing
        assert_eq!(view.state, Some(PriceState::OffPeak));
        assert_eq!(view.models[0].rows[2].active, "4");
        assert_eq!(view.models[1].rows[2].active, "13.5");
    }

    #[test]
    fn view_marks_the_current_segment_and_counts_down() {
        // Monday 09:30 Beijing: 30 minutes into the 09:00–12:00 peak.
        let view = view_at(utc(2026, 9, 14, 1, 30), 480, "CNY");
        assert_eq!(view.now_label, "09:30");
        assert_eq!(view.today_label, "Today");

        let current: Vec<&SegmentView> = view.segments.iter().filter(|s| s.is_current).collect();
        assert_eq!(current.len(), 1, "exactly one segment is current");
        assert_eq!(current[0].start_label, "09:00");
        assert_eq!(current[0].end_label, "12:00");
        assert_eq!(current[0].state_label, "Peak");

        let next = view.next_change.unwrap();
        assert_eq!(next.state_label, "Off-Peak");
        assert_eq!(next.at_label, "Today 12:00");
        assert_eq!(next.in_label, "2h 30m");
        // The tick loop sleeps on this number, so it must be the raw minutes, not the label.
        assert_eq!(next.in_minutes, 150);
    }

    #[test]
    fn the_view_reports_both_currencies_as_available() {
        let view = view_at(utc(2026, 9, 14, 12, 0), 480, "CNY");
        assert_eq!(view.available_currencies, vec!["CNY", "USD"]);
        assert_eq!(view.currency_symbol, "¥");

        let usd = view_at(utc(2026, 9, 14, 12, 0), 480, "USD");
        assert_eq!(usd.currency_symbol, "$");
        assert_eq!(usd.models[0].rows[2].active, "0.6");
    }

    #[test]
    fn a_broken_schedule_still_renders_prices_but_reports_unknown() {
        let config = crate::load_bundled().unwrap();
        let view = build_view(ViewInput {
            config: &config,
            schedule: None,
            error: Some("window 09:00–09:00 has identical start and end".to_string()),
            now: utc(2026, 9, 14, 2, 0),
            display_offset_minutes: 480,
            display_zone_label: "Asia/Shanghai".to_string(),
            currency: "CNY".to_string(),
            locale: Locale::En,
            provenance: Provenance::BuiltIn,
        });

        assert!(!view.ok);
        assert_eq!(view.state, None);
        assert_eq!(view.state_label, "");
        assert!(view.segments.is_empty());
        assert!(view.next_change.is_none());
        assert!(view.error.unwrap().contains("identical start and end"));
        // Prices remain available, because they never depended on the schedule.
        assert_eq!(view.models.len(), 2);
        assert_eq!(view.models[0].rows[2].off_peak, "4");
    }

    #[test]
    fn the_tier_note_states_what_waiting_would_save() {
        // Peak: name the current price, then the cheaper one you would get by waiting.
        let peak = view_at(utc(2026, 9, 14, 2, 0), 480, "CNY");
        assert_eq!(
            peak.tier_note,
            "Peak in effect: ¥8 / 1M output (V4.1 Flash). Off-Peak is ¥4 — waiting halves it."
        );

        // Off-peak: the same two numbers, but nothing to wait for.
        let off_peak = view_at(utc(2026, 9, 14, 12, 0), 480, "CNY");
        assert_eq!(
            off_peak.tier_note,
            "Off-Peak in effect: ¥4 / 1M output (V4.1 Flash). Peak is ¥8."
        );

        // Currency is respected.
        let usd = view_at(utc(2026, 9, 14, 2, 0), 480, "USD");
        assert!(usd.tier_note.contains("$1.2 / 1M output"));
        assert!(usd.tier_note.contains("$0.6"));
    }

    #[test]
    fn the_tier_note_states_what_waiting_would_save_in_chinese() {
        let peak = view_at_locale(utc(2026, 9, 14, 2, 0), 480, "CNY", Locale::Zh);
        assert_eq!(
            peak.tier_note,
            "当前高峰价：¥8 / 百万 tokens 输出（V4.1 Flash）。空闲价为 ¥4，等一等省一半。"
        );

        let off_peak = view_at_locale(utc(2026, 9, 14, 12, 0), 480, "CNY", Locale::Zh);
        assert_eq!(
            off_peak.tier_note,
            "当前空闲价：¥4 / 百万 tokens 输出（V4.1 Flash）。高峰价为 ¥8。"
        );
    }

    /// The whole point of the feature: a Chinese user must not see English scrolled past them.
    #[test]
    fn tooltip_renders_in_chinese() {
        let view = view_at_locale(utc(2026, 9, 14, 1, 30), 480, "CNY", Locale::Zh);
        assert_eq!(
            tooltip_lines(&view, false),
            vec![
                "DeepSeek Budget",
                "DeepSeek · V4.1 Flash",
                "高峰",
                "¥8 / 百万 tokens 输出",
                "下次空闲：今天 12:00（2小时30分）",
            ]
        );
        // The compressed Windows form too — it has its own phrasing.
        assert_eq!(
            tooltip_lines(&view, true),
            vec!["DeepSeek Budget · 高峰", "¥8 / 百万 tokens 输出", "下次空闲 今天 12:00"]
        );
    }

    /// Guards the failure mode this design actually risks: adding a field to `StateView` and
    /// forgetting to populate or translate it. A blank string is what that looks like, and it
    /// is invisible in a screenshot of the other language.
    #[test]
    fn every_locale_fills_every_user_facing_string() {
        for locale in Locale::ALL {
            let view = view_at_locale(utc(2026, 9, 14, 1, 30), 480, "CNY", locale);
            let ctx = format!("{locale:?}");

            assert!(view.ok, "{ctx}: the bundled schedule must resolve");
            assert_eq!(view.locale, locale.tag(), "{ctx}: locale tag not echoed back");
            for (name, value) in [
                ("state_label", &view.state_label),
                ("now_label", &view.now_label),
                ("today_label", &view.today_label),
                ("units_label", &view.units_label),
                ("rule_label", &view.rule_label),
                ("tier_note", &view.tier_note),
                ("offset_label", &view.offset_label),
                ("display_zone_label", &view.display_zone_label),
            ] {
                assert!(!value.is_empty(), "{ctx}: {name} is empty");
            }

            let next = view.next_change.as_ref().expect("a transition exists");
            assert!(!next.state_label.is_empty(), "{ctx}: next state_label");
            assert!(!next.at_label.is_empty(), "{ctx}: next at_label");
            assert!(!next.in_label.is_empty(), "{ctx}: next in_label");

            assert!(!view.segments.is_empty(), "{ctx}: segments");
            for segment in &view.segments {
                assert!(!segment.start_label.is_empty(), "{ctx}: segment start");
                assert!(!segment.end_label.is_empty(), "{ctx}: segment end");
                assert!(!segment.state_label.is_empty(), "{ctx}: segment state_label");
            }

            assert!(!view.models.is_empty(), "{ctx}: models");
            for model in &view.models {
                assert!(!model.label.is_empty(), "{ctx}: model label");
                assert!(!model.rows.is_empty(), "{ctx}: model rows");
                for row in &model.rows {
                    assert!(!row.label.is_empty(), "{ctx}: price row label");
                    assert!(!row.active.is_empty(), "{ctx}: price row active");
                }
            }

            for line in tooltip_lines(&view, false) {
                assert!(!line.trim().is_empty(), "{ctx}: blank tooltip line");
            }
        }
    }

    /// The test above would still pass if every Chinese string silently fell back to English.
    #[test]
    fn chinese_and_english_actually_differ_but_agree_on_the_numbers() {
        let now = utc(2026, 9, 14, 1, 30);
        let en = view_at_locale(now, 480, "CNY", Locale::En);
        let zh = view_at_locale(now, 480, "CNY", Locale::Zh);

        // Words change…
        assert_ne!(en.state_label, zh.state_label, "state_label");
        assert_ne!(en.today_label, zh.today_label, "today_label");
        assert_ne!(en.units_label, zh.units_label, "units_label");
        assert_ne!(en.rule_label, zh.rule_label, "rule_label");
        assert_ne!(en.tier_note, zh.tier_note, "tier_note");
        assert_ne!(
            en.next_change.as_ref().unwrap().in_label,
            zh.next_change.as_ref().unwrap().in_label,
            "countdown"
        );
        assert_ne!(en.models[0].rows[0].label, zh.models[0].rows[0].label, "row label");
        assert_ne!(tooltip_lines(&en, false), tooltip_lines(&zh, false), "tooltip");

        // …numbers do not.
        assert_eq!(en.models[0].rows[0].active, zh.models[0].rows[0].active, "active price");
        assert_eq!(en.now_label, zh.now_label, "clock");
        assert_eq!(
            en.segments.len(),
            zh.segments.len(),
            "segment count is language-independent"
        );
        assert_eq!(
            en.next_change.unwrap().in_minutes,
            zh.next_change.unwrap().in_minutes,
            "raw minutes drive the tick loop in either language"
        );
    }

    #[test]
    fn tier_note_is_empty_when_the_schedule_is_unknown() {
        let config = crate::load_bundled().unwrap();
        let view = build_view(ViewInput {
            config: &config,
            schedule: None,
            error: Some("boom".to_string()),
            now: utc(2026, 9, 14, 2, 0),
            display_offset_minutes: 480,
            display_zone_label: "Asia/Shanghai".to_string(),
            currency: "CNY".to_string(),
            locale: Locale::En,
            provenance: Provenance::BuiltIn,
        });
        assert_eq!(view.tier_note, "");
    }

    #[test]
    fn tooltip_macos_form_is_five_lines() {
        let view = view_at(utc(2026, 9, 14, 1, 30), 480, "CNY");
        let lines = tooltip_lines(&view, false);
        assert_eq!(
            lines,
            vec![
                "DeepSeek Budget",
                "DeepSeek · V4.1 Flash",
                "Peak",
                "¥8 / 1M output",
                "Next Off-Peak: Today 12:00 (2h 30m)",
            ]
        );
    }

    #[test]
    fn tooltip_windows_form_is_compressed() {
        let view = view_at(utc(2026, 9, 14, 1, 30), 480, "CNY");
        let lines = tooltip_lines(&view, true);
        assert_eq!(
            lines,
            vec![
                "DeepSeek Budget · Peak",
                "¥8 / 1M output",
                "Next Off-Peak Today 12:00",
            ]
        );
        // The compressed form must genuinely be shorter — that is its whole reason to exist.
        assert!(lines.len() < tooltip_lines(&view, false).len());
    }

    /// The Windows tray tooltip is capped at 127 UTF-16 code units. Overrunning it is not an
    /// error — Windows simply truncates, silently cutting the last phrase in half — so the
    /// compressed form has to be *measured* rather than assumed to be short enough. The test
    /// above only asserts it is shorter than the macOS form, which would go on passing if the
    /// compact form grew to 200 characters.
    ///
    /// UTF-16 units, not chars: Chinese is one unit per character, but the limit is counted the
    /// way Windows counts it.
    #[test]
    fn the_windows_tooltip_fits_the_platform_limit() {
        const LIMIT: usize = 127;

        for locale in Locale::ALL {
            for now in [
                utc(2026, 9, 14, 1, 30), // mid-peak: the longest next-change label
                utc(2026, 9, 14, 12, 0), // off-peak
                utc(2026, 9, 12, 2, 0),  // weekend, so the countdown spans days
            ] {
                let view = view_at_locale(now, 480, "CNY", locale);
                let tooltip = tooltip_lines(&view, true).join("\n");
                let units = tooltip.encode_utf16().count();
                assert!(
                    units <= LIMIT,
                    "{locale:?} at {now} rendered {units} UTF-16 units, over the {LIMIT} cap: {tooltip:?}"
                );
            }
        }
    }

    #[test]
    fn tooltip_says_unknown_when_the_schedule_is_broken() {
        let config = crate::load_bundled().unwrap();
        let view = build_view(ViewInput {
            config: &config,
            schedule: None,
            error: Some("boom".to_string()),
            now: utc(2026, 9, 14, 2, 0),
            display_offset_minutes: 480,
            display_zone_label: "Asia/Shanghai".to_string(),
            currency: "CNY".to_string(),
            locale: Locale::En,
            provenance: Provenance::BuiltIn,
        });
        let lines = tooltip_lines(&view, false);
        assert_eq!(lines[2], "Unknown");
        assert!(!lines.iter().any(|l| l.starts_with("Next")));
    }

    /// The provenance line is the only thing telling a user whether the figure in front of them
    /// came from the binary they installed or from a fetch. It has to be translated, present in
    /// both configs, and — on Windows — must not push the tooltip over its cap.
    #[test]
    fn the_provenance_line_names_its_source_in_both_languages() {
        let config = crate::load_bundled().unwrap();

        for (provenance, zh, en) in [
            (Provenance::BuiltIn, "内置", "Built-in"),
            (Provenance::Synced, "已同步", "Synced"),
        ] {
            for (locale, word) in [(Locale::Zh, zh), (Locale::En, en)] {
                let label = config_source_label(&config, provenance, locale);
                assert!(
                    label.starts_with(word),
                    "{provenance:?}/{locale:?} rendered {label:?}, expected it to start with {word:?}"
                );
                // The date is what makes the label actionable, so it must survive.
                assert!(
                    label.contains(&config.verified_at),
                    "{label:?} lost the verification date"
                );
            }
        }
    }

    /// A placeholder config (bundled JSON unparseable) has no date, and "Built-in · " with a
    /// dangling separator would read as a bug.
    #[test]
    fn the_provenance_line_survives_a_config_with_no_date() {
        let mut config = crate::load_bundled().unwrap();
        config.verified_at = String::new();

        assert_eq!(
            config_source_label(&config, Provenance::BuiltIn, Locale::En),
            "Built-in"
        );
    }

    /// The whole app shows prices, and every one of them is now labelled with where it came
    /// from. This pins the label into the payload rather than leaving it to the frontend.
    #[test]
    fn every_view_carries_the_provenance_label() {
        let view = view_at(utc(2026, 9, 14, 2, 0), 480, "CNY");
        assert_eq!(view.config_source_label, "Built-in · 2026-09-12");
    }
}
