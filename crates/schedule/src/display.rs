//! Pure string formatting shared by the tray tooltip and the popover.
//!
//! Kept in the engine crate (rather than the UI) so the exact strings the user sees are
//! unit-testable, and so the tooltip and popover can never disagree about them.
//!
//! Everything user-facing here takes a [`Locale`]. The Chinese forms are not literal
//! translations — they follow Chinese convention (`3小时28分` rather than `3时28分`) and use
//! DeepSeek's own vocabulary for the pricing tiers.

use crate::locale::Locale;
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};

/// Human countdown, e.g. `3h 28m` / `3小时28分`, `45m` / `45分`, `2d 15h` / `2天15小时`.
///
/// Countdowns here routinely span a weekend — Friday 18:00 Beijing to Monday 09:00 is
/// 63 hours — so days are a first-class unit, not an edge case.
pub fn humanize_duration(total_minutes: i64, locale: Locale) -> String {
    let minutes = total_minutes.max(0);
    if minutes == 0 {
        return locale.pick("不足1分钟", "<1m").to_string();
    }

    let days = minutes / (60 * 24);
    let hours = (minutes % (60 * 24)) / 60;
    let mins = minutes % 60;

    match locale {
        Locale::En => {
            if days > 0 {
                if hours > 0 {
                    format!("{days}d {hours}h")
                } else {
                    format!("{days}d")
                }
            } else if hours > 0 {
                if mins > 0 {
                    format!("{hours}h {mins}m")
                } else {
                    format!("{hours}h")
                }
            } else {
                format!("{mins}m")
            }
        }
        Locale::Zh => {
            if days > 0 {
                if hours > 0 {
                    format!("{days}天{hours}小时")
                } else {
                    format!("{days}天")
                }
            } else if hours > 0 {
                if mins > 0 {
                    format!("{hours}小时{mins}分")
                } else {
                    format!("{hours}小时")
                }
            } else {
                format!("{mins}分")
            }
        }
    }
}

/// Render minutes-from-midnight as `HH:MM`. 1440 renders as `24:00`, which is how the
/// popover's last row reads. Numeric, so it is locale-independent.
pub fn format_minute_of_day(minute: u16) -> String {
    if minute >= 1440 {
        return "24:00".to_string();
    }
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// `Today` / `Tomorrow` / a weekday name within the coming week / a date.
/// Chinese: `今天` / `明天` / `周三` / `9月21日`.
pub fn relative_day_label(target: NaiveDate, today: NaiveDate, locale: Locale) -> String {
    let delta = (target - today).num_days();
    match locale {
        Locale::En => match delta {
            0 => "Today".to_string(),
            1 => "Tomorrow".to_string(),
            -1 => "Yesterday".to_string(),
            d if (2..=6).contains(&d) => {
                crate::model::Weekday::from_chrono(target.weekday()).short(locale).to_string()
            }
            _ => target.format("%b %d").to_string(),
        },
        Locale::Zh => match delta {
            0 => "今天".to_string(),
            1 => "明天".to_string(),
            -1 => "昨天".to_string(),
            d if (2..=6).contains(&d) => {
                crate::model::Weekday::from_chrono(target.weekday()).short(locale).to_string()
            }
            _ => format!("{}月{}日", target.month(), target.day()),
        },
    }
}

/// The calendar date an instant falls on, in a zone given as a fixed UTC offset.
pub fn local_date(at_utc: DateTime<Utc>, display_offset_minutes: i32) -> NaiveDate {
    (at_utc + Duration::minutes(display_offset_minutes as i64))
        .naive_utc()
        .date()
}

/// `18:00` in the viewer's zone. Numeric, so locale-independent.
pub fn local_clock(at_utc: DateTime<Utc>, display_offset_minutes: i32) -> String {
    (at_utc + Duration::minutes(display_offset_minutes as i64))
        .naive_utc()
        .format("%H:%M")
        .to_string()
}

/// `Today 18:00` / `Mon 09:00` — or `今天 18:00` / `周一 09:00`.
pub fn local_day_and_clock(
    at_utc: DateTime<Utc>,
    display_offset_minutes: i32,
    today_local: NaiveDate,
    locale: Locale,
) -> String {
    let shifted = (at_utc + Duration::minutes(display_offset_minutes as i64)).naive_utc();
    let separator = locale.pick(" ", " ");
    format!(
        "{}{separator}{}",
        relative_day_label(shifted.date(), today_local, locale),
        shifted.format("%H:%M")
    )
}

/// `UTC+08:00`, or `UTC` at zero — used to label the popover's schedule column.
/// Numeric, so locale-independent.
/// `UTC+08:00`, `UTC-05:00`, or plain `UTC`.
///
/// **The one renderer for offsets.** `engine.rs` used to carry its own copy that padded the
/// hour differently, so a synced config without a `referenceLabel` could put `UTC+8:00` beside
/// the viewer's `UTC+08:00` in the same panel.
///
/// `unsigned_abs`, not `abs`: `i32::MIN.abs()` overflows. That panics in a debug build, and in
/// release it wraps back to `i32::MIN` and renders `UTC--35791394:-8` — a string that looks
/// like a timezone. The value arrives from a config file, i.e. from the untrusted side, so
/// neither outcome is acceptable. `compile` now rejects the range outright; this stays total
/// anyway, because a formatter that can panic is a formatter that will.
pub fn render_offset(minutes: i32) -> String {
    if minutes == 0 {
        return "UTC".to_string();
    }
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.unsigned_abs();
    format!("UTC{}{:02}:{:02}", sign, abs / 60, abs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn humanize_covers_every_unit_in_english() {
        let en = Locale::En;
        assert_eq!(humanize_duration(0, en), "<1m");
        assert_eq!(humanize_duration(-5, en), "<1m");
        assert_eq!(humanize_duration(1, en), "1m");
        assert_eq!(humanize_duration(45, en), "45m");
        assert_eq!(humanize_duration(60, en), "1h");
        assert_eq!(humanize_duration(61, en), "1h 1m");
        assert_eq!(humanize_duration(208, en), "3h 28m");
        assert_eq!(humanize_duration(1440, en), "1d");
        assert_eq!(humanize_duration(1440 + 60, en), "1d 1h");
        // The Friday-evening-to-Monday-morning gap, the longest countdown this schedule
        // can produce.
        assert_eq!(humanize_duration(63 * 60, en), "2d 15h");
    }

    #[test]
    fn humanize_covers_every_unit_in_chinese() {
        let zh = Locale::Zh;
        assert_eq!(humanize_duration(0, zh), "不足1分钟");
        assert_eq!(humanize_duration(-5, zh), "不足1分钟");
        assert_eq!(humanize_duration(1, zh), "1分");
        assert_eq!(humanize_duration(45, zh), "45分");
        assert_eq!(humanize_duration(60, zh), "1小时");
        assert_eq!(humanize_duration(61, zh), "1小时1分");
        assert_eq!(humanize_duration(208, zh), "3小时28分");
        assert_eq!(humanize_duration(1440, zh), "1天");
        assert_eq!(humanize_duration(1440 + 60, zh), "1天1小时");
        assert_eq!(humanize_duration(63 * 60, zh), "2天15小时");
    }

    /// Both languages must express the same amount of time — a translation that quietly
    /// changes the number would be worse than no translation.
    #[test]
    fn both_languages_agree_on_the_magnitude() {
        for minutes in [0, 1, 45, 60, 61, 208, 1440, 1500, 3780] {
            let en = humanize_duration(minutes, Locale::En);
            let zh = humanize_duration(minutes, Locale::Zh);
            assert!(!en.is_empty() && !zh.is_empty(), "{minutes} produced an empty string");
            assert_ne!(en, zh, "{minutes} rendered identically in both languages");
        }
    }

    #[test]
    fn minute_of_day_renders_end_of_day_as_24_00() {
        assert_eq!(format_minute_of_day(0), "00:00");
        assert_eq!(format_minute_of_day(570), "09:30");
        assert_eq!(format_minute_of_day(1439), "23:59");
        assert_eq!(format_minute_of_day(1440), "24:00");
        assert_eq!(format_minute_of_day(2000), "24:00");
    }

    #[test]
    fn relative_day_label_picks_the_right_granularity_in_english() {
        let today = date(2026, 9, 14); // Monday
        let en = Locale::En;
        assert_eq!(relative_day_label(today, today, en), "Today");
        assert_eq!(relative_day_label(date(2026, 9, 15), today, en), "Tomorrow");
        assert_eq!(relative_day_label(date(2026, 9, 13), today, en), "Yesterday");
        assert_eq!(relative_day_label(date(2026, 9, 16), today, en), "Wed");
        assert_eq!(relative_day_label(date(2026, 9, 20), today, en), "Sun");
        assert_eq!(relative_day_label(date(2026, 9, 21), today, en), "Sep 21");
    }

    #[test]
    fn relative_day_label_picks_the_right_granularity_in_chinese() {
        let today = date(2026, 9, 14); // Monday
        let zh = Locale::Zh;
        assert_eq!(relative_day_label(today, today, zh), "今天");
        assert_eq!(relative_day_label(date(2026, 9, 15), today, zh), "明天");
        assert_eq!(relative_day_label(date(2026, 9, 13), today, zh), "昨天");
        assert_eq!(relative_day_label(date(2026, 9, 16), today, zh), "周三");
        assert_eq!(relative_day_label(date(2026, 9, 20), today, zh), "周日");
        // Far enough out that a weekday name would be ambiguous — use a date instead.
        assert_eq!(relative_day_label(date(2026, 9, 21), today, zh), "9月21日");
    }

    #[test]
    fn local_clock_and_offset_are_numeric_so_locale_free() {
        let at = Utc.with_ymd_and_hms(2026, 9, 14, 1, 0, 0).unwrap();
        assert_eq!(local_clock(at, 0), "01:00");
        assert_eq!(local_clock(at, 480), "09:00");
        assert_eq!(local_clock(at, -300), "20:00");
        assert_eq!(local_date(at, -300), date(2026, 9, 13));

        assert_eq!(render_offset(0), "UTC");
        assert_eq!(render_offset(480), "UTC+08:00");
        assert_eq!(render_offset(-300), "UTC-05:00");
        assert_eq!(render_offset(330), "UTC+05:30");
    }

    #[test]
    fn day_and_clock_labels_the_day_relative_to_the_viewer() {
        let at = Utc.with_ymd_and_hms(2026, 9, 14, 1, 0, 0).unwrap();
        assert_eq!(
            local_day_and_clock(at, 480, date(2026, 9, 14), Locale::En),
            "Today 09:00"
        );
        // Same instant, a viewer for whom it is still Sunday evening.
        assert_eq!(
            local_day_and_clock(at, -300, date(2026, 9, 13), Locale::En),
            "Today 20:00"
        );
        assert_eq!(
            local_day_and_clock(at, 480, date(2026, 9, 14), Locale::Zh),
            "今天 09:00"
        );
    }
}
