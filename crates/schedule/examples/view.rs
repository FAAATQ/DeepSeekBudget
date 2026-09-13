//! Print exactly what the tray tooltip and the popover would show at a given instant.
//!
//! This is the offline half of the time-travel workflow. `DEEPSEEKBUDGET_FAKE_NOW` shows you the
//! real tray, which is the honest check but needs a GUI and a click; this prints the same
//! content as text, so the wording, the countdowns and the price rounding can be eyeballed —
//! or diffed in a test — without launching anything.
//!
//! ```text
//! cargo run -p deepseekbudget-schedule --example view -- 2026-09-14T01:30:00Z
//! cargo run -p deepseekbudget-schedule --example view -- 2026-09-14T05:00:00Z 480 USD
//! ```
//!
//! Arguments:
//! `[--json] [--broken] [--synced] [<RFC3339 instant>] [display offset minutes] [currency] [lang]`.
//!
//! `--json` prints the `StateView` payload the popover consumes, which
//! `tools/make-preview.py` feeds into a headless-Chrome render of the real UI.
//! `--broken` simulates a schedule that failed to compile, for previewing the Unknown state.
//! `--synced` labels the figures as coming from a fetched copy rather than the binary.
//! `lang` is `zh` or `en` (anything else falls back to English).

use deepseekbudget_schedule::{
    build_view, display::render_offset, load_bundled, tooltip_lines, CompiledSchedule, Locale,
    Provenance, ViewInput,
};
use chrono::{DateTime, Utc};

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let json_mode = raw.iter().any(|a| a == "--json");
    let broken = raw.iter().any(|a| a == "--broken");
    let synced = raw.iter().any(|a| a == "--synced");

    let mut positional = raw.into_iter().filter(|a| !a.starts_with("--"));
    let instant = positional
        .next()
        .unwrap_or_else(|| "2026-09-14T01:30:00Z".to_string());
    let offset: i32 = positional.next().and_then(|raw| raw.parse().ok()).unwrap_or(480);
    let currency = positional.next().unwrap_or_else(|| "CNY".to_string());
    let locale = Locale::from_tag(&positional.next().unwrap_or_else(|| "en".to_string()));

    let now = match DateTime::parse_from_rfc3339(&instant) {
        Ok(parsed) => parsed.with_timezone(&Utc),
        Err(e) => {
            eprintln!("could not parse {instant:?} as RFC 3339: {e}");
            std::process::exit(2);
        }
    };

    let config = load_bundled().expect("bundled provider config must parse");
    let (schedule, error) = if broken {
        (
            None,
            Some("window 09:00–09:00 has identical start and end; fix the typo".to_string()),
        )
    } else {
        (
            Some(
                CompiledSchedule::compile(&config.schedule)
                    .expect("bundled schedule must compile"),
            ),
            None,
        )
    };

    let view = build_view(ViewInput {
        config: &config,
        schedule: schedule.as_ref(),
        error,
        now,
        display_offset_minutes: offset,
        display_zone_label: render_offset(offset),
        currency: currency.clone(),
        locale,
        provenance: if synced {
            Provenance::Synced
        } else {
            Provenance::BuiltIn
        },
    });

    if json_mode {
        println!("{}", serde_json::to_string(&view).expect("StateView serializes"));
        return;
    }

    println!(
        "=== {instant}  (display {} , {currency}, {}) ===",
        render_offset(offset),
        locale.tag()
    );
    println!(
        "state: {}   now: {}   zone: {}",
        view.state_label, view.now_label, view.display_zone_label
    );
    if let Some(next) = &view.next_change {
        println!(
            "next: {} at {} (in {}, {} raw minutes)",
            next.state_label, next.at_label, next.in_label, next.in_minutes
        );
    }

    println!("\n--- macOS tooltip ---");
    for line in tooltip_lines(&view, false) {
        println!("{line}");
    }
    println!("\n--- Windows tooltip (compressed) ---");
    for line in tooltip_lines(&view, true) {
        println!("{line}");
    }

    println!("\n--- popover: {} ---", view.today_label);
    for segment in &view.segments {
        println!(
            "  {}–{}  {}{}",
            segment.start_label,
            segment.end_label,
            segment.state_label,
            if segment.is_current { "   ◀ now" } else { "" }
        );
    }
    println!("  published rule: {}", view.rule_label);

    println!("\n--- popover: prices ({} per 1M tokens) ---", view.currency_symbol);
    print!("  {:<20}", "");
    for model in &view.models {
        print!("{:>10}", model.label);
    }
    println!();
    if let Some(first) = view.models.first() {
        for (i, row) in first.rows.iter().enumerate() {
            print!("  {:<20}", row.label);
            for model in &view.models {
                let value = model
                    .rows
                    .get(i)
                    .map(|r| r.active.clone())
                    .unwrap_or_else(|| "—".to_string());
                print!("{value:>10}");
            }
            println!();
        }
    }
    println!("  tier note: {} prices in effect.", view.state_label);
}
