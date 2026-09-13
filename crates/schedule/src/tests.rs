//! Boundary tests for the scheduling engine.
//!
//! These exist because the alternative way to check "does the icon turn orange at 09:00?" is
//! to wait until 09:00. Every rule the real DeepSeek schedule exercises — two windows a day,
//! weekdays only, a 63-hour weekend gap, half-open boundaries — is pinned here instead.
//!
//! The bundled config is the test fixture for most of these, so a transcription error in
//! `deepseek.json` fails the suite rather than shipping.

use crate::display::{format_minute_of_day, humanize_duration, local_clock, local_date};
use crate::engine::{CompiledSchedule, PriceState, parse_fake_now};
use crate::locale::Locale;
use crate::model::{ScheduleConfig, ScheduleError, Weekday, WeeklyRule};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// The shipped DeepSeek schedule.
fn ds() -> CompiledSchedule {
    crate::bundled_schedule().expect("bundled config must compile")
}

/// A one-rule schedule with a fixed UTC reference zone, for cases the real config can't reach
/// (wrapping windows, offsets that split a window across local midnight).
fn synthetic(days: Vec<Weekday>, windows: Vec<(&str, &str)>) -> CompiledSchedule {
    CompiledSchedule::compile(&ScheduleConfig {
        reference_utc_offset_minutes: 0,
        reference_label: Some("UTC".to_string()),
        weekly: vec![WeeklyRule {
            days,
            windows: windows
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }],
    })
    .expect("synthetic config must compile")
}

fn minutes_of(segs: &[crate::engine::DaySegment]) -> Vec<(u16, u16, PriceState)> {
    segs.iter()
        .map(|s| (s.start_minute, s.end_minute, s.state))
        .collect()
}

// ---------------------------------------------------------------------------
// Calendar anchors
// ---------------------------------------------------------------------------

/// Guards the hand-computed dates the rest of this file relies on.
#[test]
fn calendar_anchors_are_what_these_tests_assume() {
    use chrono::Weekday as C;
    assert_eq!(date(2026, 9, 11).weekday(), C::Fri);
    assert_eq!(date(2026, 9, 12).weekday(), C::Sat);
    assert_eq!(date(2026, 9, 13).weekday(), C::Sun);
    assert_eq!(date(2026, 9, 14).weekday(), C::Mon);
    assert_eq!(date(2026, 12, 31).weekday(), C::Thu);
    assert_eq!(date(2027, 1, 1).weekday(), C::Fri);
    assert_eq!(date(2027, 1, 2).weekday(), C::Sat);
}

// ---------------------------------------------------------------------------
// The bundled config itself
// ---------------------------------------------------------------------------

#[test]
fn bundled_config_matches_the_official_pricing_page() {
    let cfg = crate::load_bundled().unwrap();
    assert_eq!(cfg.provider, "deepseek");
    assert_eq!(cfg.display_name, "DeepSeek");
    assert!(cfg.source_url.starts_with("https://api-docs.deepseek.com"));
    assert_eq!(cfg.verified_at, "2026-09-12");
    assert_eq!(cfg.default_currency, "CNY");

    // "Peak hours are 01:00 - 04:00 and 06:00 - 10:00 UTC, Monday through Friday."
    let sched = &cfg.schedule;
    assert_eq!(sched.reference_utc_offset_minutes, 0);
    assert_eq!(sched.weekly.len(), 1);
    let rule = &sched.weekly[0];
    assert_eq!(
        rule.days,
        vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri
        ]
    );
    assert_eq!(
        rule.windows,
        vec![
            ("01:00".to_string(), "04:00".to_string()),
            ("06:00".to_string(), "10:00".to_string())
        ]
    );
}

#[test]
fn bundled_model_ids_and_prices_match_the_official_page() {
    let cfg = crate::load_bundled().unwrap();
    assert_eq!(cfg.models.len(), 2);

    let flash = cfg
        .models
        .iter()
        .find(|m| m.id == "deepseek-flash")
        .expect("deepseek-flash is the current Flash model id");
    assert_eq!(flash.version.as_deref(), Some("DeepSeek-V4.1-Flash"));
    let p = flash.price_in("CNY").unwrap();
    assert_eq!(p.input_cache_hit.off_peak, 0.02);
    assert_eq!(p.input_cache_hit.peak, 0.04);
    assert_eq!(p.input_cache_miss.off_peak, 1.0);
    assert_eq!(p.input_cache_miss.peak, 2.0);
    assert_eq!(p.output.off_peak, 4.0);
    assert_eq!(p.output.peak, 8.0);
    let pu = flash.price_in("USD").unwrap();
    assert_eq!(pu.input_cache_hit.off_peak, 0.003);
    assert_eq!(pu.input_cache_miss.peak, 0.3);
    assert_eq!(pu.output.peak, 1.2);

    let pro = cfg
        .models
        .iter()
        .find(|m| m.id == "deepseek-v4-pro")
        .expect("deepseek-v4-pro");
    assert_eq!(pro.version.as_deref(), Some("DeepSeek-V4-Pro-0813"));
    let q = pro.price_in("CNY").unwrap();
    assert_eq!(q.input_cache_hit.peak, 0.3);
    assert_eq!(q.input_cache_miss.off_peak, 4.5);
    assert_eq!(q.output.peak, 27.0);
    let qu = pro.price_in("USD").unwrap();
    assert_eq!(qu.output.off_peak, 1.98);
    assert_eq!(qu.output.peak, 3.96);
}

/// "Off-peak rates are half of the peak rates." Cheap to assert, and it catches any
/// transcription slip that happens to look plausible.
#[test]
fn every_transcribed_price_obeys_the_half_price_rule() {
    let cfg = crate::load_bundled().unwrap();
    for model in &cfg.models {
        for (currency, prices) in &model.prices {
            for (name, tier) in [
                ("inputCacheHit", prices.input_cache_hit),
                ("inputCacheMiss", prices.input_cache_miss),
                ("output", prices.output),
            ] {
                assert_eq!(
                    tier.off_peak * 2.0,
                    tier.peak,
                    "{}/{}/{} is not half price off-peak",
                    model.id,
                    currency,
                    name
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Boundary behaviour
// ---------------------------------------------------------------------------

#[test]
fn first_window_starts_peak_and_ends_half_open() {
    let s = ds();
    // Friday 2026-09-11, a weekday.
    assert_eq!(s.state_at(utc(2026, 9, 11, 0, 59, 59)), PriceState::OffPeak);
    assert_eq!(s.state_at(utc(2026, 9, 11, 1, 0, 0)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 11, 2, 30, 0)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 11, 3, 59, 59)), PriceState::Peak);
    // [start, end) — the end instant already belongs to the next tier.
    assert_eq!(s.state_at(utc(2026, 9, 11, 4, 0, 0)), PriceState::OffPeak);
}

#[test]
fn the_gap_between_the_two_windows_is_off_peak() {
    let s = ds();
    assert_eq!(s.state_at(utc(2026, 9, 11, 4, 30, 0)), PriceState::OffPeak);
    assert_eq!(s.state_at(utc(2026, 9, 11, 5, 59, 59)), PriceState::OffPeak);
}

#[test]
fn second_window_starts_peak_and_ends_half_open() {
    let s = ds();
    assert_eq!(s.state_at(utc(2026, 9, 11, 6, 0, 0)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 11, 9, 59, 59)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 11, 10, 0, 0)), PriceState::OffPeak);
}

#[test]
fn weekends_are_entirely_off_peak() {
    let s = ds();
    for hour in [0, 1, 2, 3, 4, 6, 8, 9, 10, 12, 18, 23] {
        assert_eq!(
            s.state_at(utc(2026, 9, 12, hour, 0, 0)),
            PriceState::OffPeak,
            "Saturday {hour}:00 should be off-peak"
        );
        assert_eq!(
            s.state_at(utc(2026, 9, 13, hour, 30, 0)),
            PriceState::OffPeak,
            "Sunday {hour}:30 should be off-peak"
        );
    }
    assert_eq!(s.day_segments(date(2026, 9, 12), 480).len(), 1);
}

#[test]
fn friday_close_to_monday_open_is_a_63_hour_gap() {
    let s = ds();
    let friday_close = utc(2026, 9, 11, 10, 0, 0);
    assert_eq!(s.state_at(friday_close), PriceState::OffPeak);

    let next = s.next_transition(friday_close).unwrap();
    assert_eq!(next.state, PriceState::Peak);
    assert_eq!(next.at_utc, utc(2026, 9, 14, 1, 0, 0));
    assert_eq!((next.at_utc - friday_close).num_minutes(), 63 * 60);
    assert_eq!(humanize_duration(63 * 60, Locale::En), "2d 15h");
    assert_eq!(humanize_duration(63 * 60, Locale::Zh), "2天15小时");
}

#[test]
fn monday_open_resumes_peak() {
    let s = ds();
    assert_eq!(s.state_at(utc(2026, 9, 14, 0, 59, 59)), PriceState::OffPeak);
    assert_eq!(s.state_at(utc(2026, 9, 14, 1, 0, 0)), PriceState::Peak);
}

/// The single most important correctness rule in the engine: day-of-week is judged in the
/// schedule's reference zone, not the viewer's.
#[test]
fn weekday_is_judged_in_the_reference_zone_not_the_viewers() {
    let s = ds();
    let monday_0100_utc = utc(2026, 9, 14, 1, 0, 0);

    // A UTC-5 viewer sees this instant on Sunday evening...
    assert_eq!(local_date(monday_0100_utc, -300), date(2026, 9, 13));
    assert_eq!(local_clock(monday_0100_utc, -300), "20:00");

    // ...and it is peak anyway, because Monday 01:00 UTC is what the provider published.
    assert_eq!(s.state_at(monday_0100_utc), PriceState::Peak);
}

#[test]
fn year_boundary_does_not_disturb_the_schedule() {
    let s = ds();
    assert_eq!(s.state_at(utc(2026, 12, 31, 2, 0, 0)), PriceState::Peak); // Thursday
    assert_eq!(s.state_at(utc(2027, 1, 1, 2, 0, 0)), PriceState::Peak); // Friday
    assert_eq!(s.state_at(utc(2027, 1, 2, 2, 0, 0)), PriceState::OffPeak); // Saturday
}

#[test]
fn a_wrapping_window_stays_peak_past_midnight() {
    // Monday 22:00 → Tuesday 02:00. The tail belongs to Monday's rule.
    let s = synthetic(vec![Weekday::Mon], vec![("22:00", "02:00")]);
    assert_eq!(s.state_at(utc(2026, 9, 14, 21, 59, 59)), PriceState::OffPeak);
    assert_eq!(s.state_at(utc(2026, 9, 14, 22, 0, 0)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 15, 1, 0, 0)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 15, 2, 0, 0)), PriceState::OffPeak);
    // The wrap does not leak into the following Monday's rule on Wednesday.
    assert_eq!(s.state_at(utc(2026, 9, 16, 23, 0, 0)), PriceState::OffPeak);
}

// ---------------------------------------------------------------------------
// Transition reporting
// ---------------------------------------------------------------------------

#[test]
fn the_tier_flips_exactly_at_the_reported_transition() {
    let s = ds();
    let mut t = utc(2026, 9, 10, 12, 0, 0);
    for _ in 0..24 {
        let before = s.state_at(t);
        let next = s.next_transition(t).expect("a transition must exist");

        assert_eq!(
            next.state,
            before.opposite(),
            "a change must go to the other tier"
        );
        assert_eq!(
            s.state_at(next.at_utc),
            next.state,
            "state at the boundary must be the new tier"
        );
        assert_eq!(
            s.state_at(next.at_utc - Duration::seconds(1)),
            before,
            "the instant before the boundary must still be the old tier"
        );

        t = next.at_utc + Duration::seconds(1);
    }
}

#[test]
fn snapshot_reports_state_and_countdown() {
    let s = ds();
    // Monday 01:30 UTC = 09:30 Beijing, half an hour into the morning peak.
    let snap = s.snapshot(utc(2026, 9, 14, 1, 30, 0));
    assert_eq!(snap.state, PriceState::Peak);
    let next = snap.next_change.unwrap();
    assert_eq!(next.at_utc, utc(2026, 9, 14, 4, 0, 0)); // 12:00 Beijing
    assert_eq!(next.state, PriceState::OffPeak);
    assert_eq!(snap.minutes_until_change, Some(150));
}

#[test]
fn snapshot_serializes_with_camel_case_keys_for_the_ui() {
    let s = ds();
    let json = serde_json::to_value(s.snapshot(utc(2026, 9, 14, 1, 30, 0))).unwrap();
    assert_eq!(json["state"], "peak");
    assert_eq!(json["nextChange"]["state"], "off-peak");
    assert_eq!(json["minutesUntilChange"], 150);
}

// ---------------------------------------------------------------------------
// The popover's day view
// ---------------------------------------------------------------------------

/// The exact list the popover renders on a weekday in Beijing — including the 12:00–14:00
/// lunch break, which is off-peak.
#[test]
fn monday_in_beijing_matches_the_designed_popover() {
    let s = ds();
    let segments = s.day_segments(date(2026, 9, 14), 480);
    let rendered: Vec<(String, String, &str)> = segments
        .iter()
        .map(|seg| {
            (
                format_minute_of_day(seg.start_minute),
                format_minute_of_day(seg.end_minute),
                seg.state.label(Locale::En),
            )
        })
        .collect();

    assert_eq!(
        rendered,
        vec![
            ("00:00".to_string(), "09:00".to_string(), "Off-Peak"),
            ("09:00".to_string(), "12:00".to_string(), "Peak"),
            ("12:00".to_string(), "14:00".to_string(), "Off-Peak"),
            ("14:00".to_string(), "18:00".to_string(), "Peak"),
            ("18:00".to_string(), "24:00".to_string(), "Off-Peak"),
        ]
    );
}

/// Same schedule, Chinese labels — and the times must be untouched by the translation.
/// Uses DeepSeek's own tier vocabulary (高峰 / 空闲), not a literal translation.
#[test]
fn monday_in_beijing_renders_in_chinese_with_identical_times() {
    let s = ds();
    let segments = s.day_segments(date(2026, 9, 14), 480);
    let rendered: Vec<(String, String, &str)> = segments
        .iter()
        .map(|seg| {
            (
                format_minute_of_day(seg.start_minute),
                format_minute_of_day(seg.end_minute),
                seg.state.label(Locale::Zh),
            )
        })
        .collect();

    assert_eq!(
        rendered,
        vec![
            ("00:00".to_string(), "09:00".to_string(), "空闲"),
            ("09:00".to_string(), "12:00".to_string(), "高峰"),
            ("12:00".to_string(), "14:00".to_string(), "空闲"),
            ("14:00".to_string(), "18:00".to_string(), "高峰"),
            ("18:00".to_string(), "24:00".to_string(), "空闲"),
        ]
    );
}

#[test]
fn a_window_crossing_local_midnight_is_split_across_two_local_days() {
    // Monday 14:00–18:00 UTC is Monday 22:00 → Tuesday 02:00 in UTC+8.
    let s = synthetic(vec![Weekday::Mon], vec![("14:00", "18:00")]);

    assert_eq!(
        minutes_of(&s.day_segments(date(2026, 9, 14), 480)),
        vec![
            (0, 1320, PriceState::OffPeak),
            (1320, 1440, PriceState::Peak),
        ]
    );
    assert_eq!(
        minutes_of(&s.day_segments(date(2026, 9, 15), 480)),
        vec![
            (0, 120, PriceState::Peak),
            (120, 1440, PriceState::OffPeak),
        ]
    );
}

/// Invariant: a day is always tiled by alternating segments from 00:00 to 24:00, in every
/// display zone, for a whole year.
#[test]
fn day_segments_always_tile_the_whole_day() {
    let s = ds();
    let mut day = date(2026, 1, 1);
    let end = date(2027, 1, 1);
    let mut days_with_peak = 0;

    while day < end {
        for offset in [0, 480, -300, 330, -660] {
            let segments = s.day_segments(day, offset);
            assert!(!segments.is_empty(), "{day} @{offset}: no segments");
            assert_eq!(
                segments.first().unwrap().start_minute,
                0,
                "{day} @{offset}: does not start at midnight"
            );
            assert_eq!(
                segments.last().unwrap().end_minute,
                1440,
                "{day} @{offset}: does not end at midnight"
            );
            for seg in &segments {
                assert!(
                    seg.start_minute < seg.end_minute,
                    "{day} @{offset}: empty segment {seg:?}"
                );
            }
            for pair in segments.windows(2) {
                assert_eq!(
                    pair[0].end_minute, pair[1].start_minute,
                    "{day} @{offset}: gap or overlap between {pair:?}"
                );
                assert_ne!(
                    pair[0].state, pair[1].state,
                    "{day} @{offset}: adjacent segments share a tier, should have merged"
                );
            }
            if offset == 480 && segments.iter().any(|seg| seg.state == PriceState::Peak) {
                days_with_peak += 1;
            }
        }
        day += Duration::days(1);
    }

    // 2026 has 261 weekdays; every one of them has peak segments in Beijing time.
    assert!(
        days_with_peak > 250,
        "expected most weekdays to contain peak time, got {days_with_peak}"
    );
}

/// Invariant: `state_at` agrees with the generated interval set across a whole year. This is
/// what catches wrap-around, merge and offset bugs that individual cases miss.
#[test]
fn state_agrees_with_the_generated_windows_across_a_year() {
    let s = ds();
    let windows = s.peak_windows_utc(date(2025, 12, 30), 372);
    let mut t = utc(2026, 1, 1, 0, 0, 0);
    let end = utc(2027, 1, 1, 0, 0, 0);
    let (mut peak, mut off_peak) = (0usize, 0usize);

    while t < end {
        let expected = if windows.iter().any(|(a, b)| *a <= t && t < *b) {
            PriceState::Peak
        } else {
            PriceState::OffPeak
        };
        assert_eq!(s.state_at(t), expected, "mismatch at {t}");
        match expected {
            PriceState::Peak => peak += 1,
            PriceState::OffPeak => off_peak += 1,
        }
        t += Duration::minutes(7);
    }

    assert!(peak > 0 && off_peak > 0, "sweep never saw both tiers");
}

#[test]
fn leap_day_and_month_boundaries_produce_a_well_formed_day() {
    let s = ds();
    for day in [date(2028, 2, 28), date(2028, 2, 29), date(2028, 3, 1)] {
        assert!(day.leap_year() || day.month() == 3);
        let segments = s.day_segments(day, 480);
        assert_eq!(segments.first().unwrap().start_minute, 0, "{day}");
        assert_eq!(segments.last().unwrap().end_minute, 1440, "{day}");
    }
}

// ---------------------------------------------------------------------------
// Config validation — these are what turn a broken config into the Unknown state
// ---------------------------------------------------------------------------

#[test]
fn zero_length_window_is_rejected() {
    let cfg = ScheduleConfig {
        reference_utc_offset_minutes: 0,
        reference_label: None,
        weekly: vec![WeeklyRule {
            days: vec![Weekday::Mon],
            windows: vec![("09:00".to_string(), "09:00".to_string())],
        }],
    };
    assert!(matches!(
        CompiledSchedule::compile(&cfg),
        Err(ScheduleError::ZeroLengthWindow { .. })
    ));
}

#[test]
fn malformed_times_are_rejected() {
    for bad in ["9am", "25:00", "", "01:0"] {
        let cfg = ScheduleConfig {
            reference_utc_offset_minutes: 0,
            reference_label: None,
            weekly: vec![WeeklyRule {
                days: vec![Weekday::Mon],
                windows: vec![(bad.to_string(), "10:00".to_string())],
            }],
        };
        assert!(
            matches!(
                CompiledSchedule::compile(&cfg),
                Err(ScheduleError::BadTime { .. })
            ),
            "{bad:?} should be rejected"
        );
    }
}

#[test]
fn empty_schedules_are_rejected() {
    let no_rules = ScheduleConfig {
        reference_utc_offset_minutes: 0,
        reference_label: None,
        weekly: vec![],
    };
    assert!(matches!(
        CompiledSchedule::compile(&no_rules),
        Err(ScheduleError::NoRules)
    ));

    let no_windows = ScheduleConfig {
        reference_utc_offset_minutes: 0,
        reference_label: None,
        weekly: vec![WeeklyRule {
            days: vec![Weekday::Mon],
            windows: vec![],
        }],
    };
    assert!(matches!(
        CompiledSchedule::compile(&no_windows),
        Err(ScheduleError::EmptyWindows)
    ));
}

#[test]
fn seconds_precision_times_are_accepted() {
    let s = synthetic(vec![Weekday::Mon], vec![("01:30:15", "02:45:30")]);
    assert_eq!(s.state_at(utc(2026, 9, 14, 1, 30, 15)), PriceState::Peak);
    assert_eq!(s.state_at(utc(2026, 9, 14, 1, 30, 14)), PriceState::OffPeak);
    assert_eq!(s.state_at(utc(2026, 9, 14, 2, 45, 30)), PriceState::OffPeak);
}

// ---------------------------------------------------------------------------
// The dev time-travel hook
// ---------------------------------------------------------------------------

#[test]
fn fake_now_parsing_accepts_rfc3339_and_normalises_to_utc() {
    assert!(parse_fake_now("").unwrap().is_none());
    assert!(parse_fake_now("   ").unwrap().is_none());

    let pinned = parse_fake_now("2026-09-14T01:00:00Z").unwrap().unwrap();
    assert_eq!(pinned.0, utc(2026, 9, 14, 1, 0, 0));

    // Same instant written in Beijing time.
    let beijing = parse_fake_now("2026-09-14T09:00:00+08:00").unwrap().unwrap();
    assert_eq!(beijing.0, utc(2026, 9, 14, 1, 0, 0));
}

#[test]
fn fake_now_parsing_rejects_nonsense_with_a_useful_message() {
    let err = parse_fake_now("yesterday afternoon").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("DEEPSEEKBUDGET_FAKE_NOW"));
    assert!(message.contains("RFC 3339"));
}
