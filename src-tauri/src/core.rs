//! Application state and the commands the popover calls.
//!
//! Everything here is a thin shell over `deepseekbudget-schedule`. The rule this module follows:
//! **no failure may remove the tray icon.** A missing config, an unparseable schedule or a
//! bad `DEEPSEEKBUDGET_FAKE_NOW` all resolve to the grey Unknown dot plus an error string, never
//! to a panic or an absent icon.

use deepseekbudget_schedule::{
    Clock, CompiledSchedule, Locale, PriceState, Provenance, ProviderConfig, StateView, ViewInput,
    build_view, clock_from_env, display, load_bundled, tooltip_lines,
};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, State};

use crate::provider;

pub const APP_NAME: &str = "DeepSeek Budget";
const SETTINGS_FILE: &str = "settings.json";

/// Fixed offsets offered in the settings panel.
///
/// Deliberately a short list rather than a full zone database. V0.1 models the display zone
/// as a fixed offset from UTC, which is exact for zones without DST — including the +08:00
/// audience this app is built for — and a documented approximation elsewhere. "System
/// default" is the recommended choice and is what almost everyone should use.
const TIMEZONE_CHOICES: &[i32] = &[0, 480, 540, 420, 330, 60, -300, -480, -600];

/// The intervals offered for automatic price checks, in hours. `0` is off.
///
/// A short list rather than a free-text field, for the same reason the timezone list is short:
/// the useful answers are few, and a picker cannot be given a value the app has to defend
/// against. Daily is the default — the figures are published by someone else, and the whole point
/// of this app is not to be looking at yesterday's price.
const AUTO_CHECK_HOURS: &[u32] = &[0, 6, 12, 24, 72];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Currency code to display prices in. Empty means "use the provider's default".
    #[serde(default)]
    pub currency: String,
    /// Display timezone override, in minutes from UTC. `None` follows the system zone.
    #[serde(default)]
    pub timezone_override_minutes: Option<i32>,
    /// Interface language tag (`"zh"` / `"en"`). `None` follows the system language.
    #[serde(default)]
    pub language: Option<String>,
    /// Hours between automatic price checks. `0` turns them off.
    ///
    /// Defaults to 24. The figures are published by someone else, and going stale is the one
    /// failure this app exists to prevent — so the app goes and looks, rather than waiting to be
    /// asked. This is a deliberate revision of hard constraint 5, which used to read "only a user
    /// click sends a request". **Its structural half is untouched**: the request still goes
    /// through the webview's `fetch`, the Rust tree still contains no HTTP client, and the CSP
    /// still permits exactly one origin. What changed is who decides *when*. See
    /// docs/design/architecture.md §7 for the cost that was accepted.
    #[serde(default = "default_auto_check_hours")]
    pub auto_check_hours: u32,
    /// When the last automatic check finished, as RFC 3339. `None` means never — and never
    /// counts as due, so a fresh install gets today's figures instead of waiting a day.
    #[serde(default)]
    pub last_auto_check: Option<String>,
}

/// The default interval, named so `serde` and `Default` cannot drift apart.
fn default_auto_check_hours() -> u32 {
    24
}

/// How long to wait between automatic checks.
///
/// Development affordance, in the same family as `DEEPSEEKBUDGET_FAKE_NOW`: without it this
/// feature can only be tested by waiting a day, which means it would never be tested. Takes a
/// number of **seconds** — `DEEPSEEKBUDGET_AUTOCHECK_SECONDS=5` makes the whole path observable
/// in five seconds. A typo is warned about rather than obeyed, like every other one of these.
pub const AUTOCHECK_ENV: &str = "DEEPSEEKBUDGET_AUTOCHECK_SECONDS";

/// The interval in force, environment override included. A zero interval means "off", which is
/// how the setting spells it too.
pub fn auto_check_interval(hours: u32) -> chrono::Duration {
    if let Ok(raw) = std::env::var(AUTOCHECK_ENV) {
        match raw.trim().parse::<i64>() {
            Ok(seconds) if seconds >= 0 => return chrono::Duration::seconds(seconds),
            _ => eprintln!("{APP_NAME}: ignoring {AUTOCHECK_ENV}=\"{raw}\" — expected a number"),
        }
    }
    if hours == 0 {
        return chrono::Duration::zero();
    }
    chrono::Duration::hours(i64::from(hours))
}

/// Whether an automatic check is due.
///
/// Pure — `now` is a parameter, never read from the clock — so the boundary can be tested without
/// waiting a day, for the same reason `engine::state_at` takes `now`. A non-positive interval is
/// off; a check that has never run is always due, so a fresh install gets today's figures rather
/// than waiting a day for them.
pub fn auto_check_due(now: DateTime<Utc>, last: Option<&str>, interval: chrono::Duration) -> bool {
    if interval <= chrono::Duration::zero() {
        return false;
    }
    match last.and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok()) {
        None => true,
        Some(when) => now.signed_duration_since(when.with_timezone(&Utc)) >= interval,
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            currency: String::new(),
            timezone_override_minutes: None,
            language: None,
            auto_check_hours: default_auto_check_hours(),
            last_auto_check: None,
        }
    }
}

impl Settings {
    fn load(dir: &Path) -> Option<Settings> {
        let raw = std::fs::read_to_string(dir.join(SETTINGS_FILE)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(dir.join(SETTINGS_FILE), json)
    }
}

/// What was last pushed to the tray. Used to skip redundant `set_icon`/`set_tooltip` calls —
/// those are the only expensive part of a refresh, and doing them unchanged can flicker.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rendered {
    pub state: Option<PriceState>,
    pub tooltip: String,
}

pub struct AppCore {
    pub config: ProviderConfig,
    /// Which copy of the figures is in force — the one in the binary, or one the user synced.
    pub provenance: Provenance,
    /// `None` when the schedule could not be compiled; the UI then shows Unknown.
    pub schedule: Option<CompiledSchedule>,
    pub error: Option<String>,
    /// A synced copy that was found on disk and refused. Non-fatal: the bundled config is in
    /// force and the app is fine. Surfaced in settings so a user whose file was ignored is told
    /// why rather than left guessing.
    pub provider_notice: Option<String>,
    pub clock: Box<dyn Clock>,
    pub settings: Settings,
    /// Resolved interface language — the override if set, else the system's.
    pub locale: Locale,
    /// Whether the native window material was actually applied. The popover's CSS falls back
    /// to an opaque background when this is false, rather than pretending glass is there and
    /// leaving 10pt text unreadable over a wallpaper.
    pub glass_applied: bool,
    /// A scheduled check the tick loop asked for and the webview has not collected yet.
    ///
    /// In memory only — it describes "right now", not a user preference. See [`take_auto_check`]
    /// for why this exists instead of relying on the event alone.
    pub auto_check_pending: bool,
    pub rendered: Rendered,
    settings_dir: PathBuf,
}

impl AppCore {
    pub fn load(settings_dir: PathBuf) -> Self {
        // Which config is in force — the bundled one, or a synced copy the engine accepted — is
        // `provider`'s decision, including every way it can go wrong. It never fails: the worst
        // case is the placeholder plus an error string.
        let provider::Loaded {
            config,
            provenance,
            mut error,
            notice,
        } = provider::load(&settings_dir);

        let schedule = match CompiledSchedule::compile(&config.schedule) {
            Ok(schedule) => Some(schedule),
            Err(e) => {
                error = Some(e.to_string());
                None
            }
        };

        // A malformed DEEPSEEKBUDGET_FAKE_NOW is a developer typo, not a product failure. Warn on
        // stderr — the channel a developer is actually watching — and carry on with the real
        // clock. Routing it into `error` would pin the tray to a grey Unknown dot for end
        // users who could never have set this variable in the first place.
        let clock = match clock_from_env() {
            Ok(clock) => clock,
            Err(e) => {
                eprintln!("{APP_NAME}: ignoring the pinned clock — {e}");
                Box::new(deepseekbudget_schedule::SystemClock)
            }
        };

        let mut settings = Settings::load(&settings_dir).unwrap_or_default();
        let available: Vec<String> = available_currencies(&config);
        if !available.contains(&settings.currency) {
            settings.currency = config.default_currency.clone();
        }
        let locale = resolve_locale(&settings);

        AppCore {
            config,
            provenance,
            schedule,
            error,
            provider_notice: notice,
            clock,
            settings,
            locale,
            auto_check_pending: false,
            // Flipped to true by `popover` if the native material applies. Defaults to the
            // honest answer: not applied yet.
            glass_applied: false,
            rendered: Rendered::default(),
            settings_dir,
        }
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now_utc()
    }

    /// The viewer's UTC offset in minutes: the override if set, otherwise the system zone.
    pub fn display_offset_minutes(&self) -> i32 {
        self.settings
            .timezone_override_minutes
            .unwrap_or_else(system_offset_minutes)
    }

    /// Human label for the zone the schedule list is rendered in.
    pub fn zone_label(&self) -> String {
        if self.settings.timezone_override_minutes.is_some() {
            return display::render_offset(self.display_offset_minutes());
        }
        system_zone_name().unwrap_or_else(|| display::render_offset(self.display_offset_minutes()))
    }

    pub fn view(&self) -> StateView {
        build_view(ViewInput {
            config: &self.config,
            schedule: self.schedule.as_ref(),
            error: self.error.clone(),
            now: self.now(),
            display_offset_minutes: self.display_offset_minutes(),
            display_zone_label: self.zone_label(),
            currency: self.settings.currency.clone(),
            locale: self.locale,
            provenance: self.provenance,
        })
    }

    /// Swap in a config, and everything that has to stay true when one changes.
    ///
    /// Kept in one place because "the config changed" is not just an assignment: the schedule
    /// has to be recompiled, and the user's currency may no longer exist. A caller that set
    /// `self.config` directly would leave a schedule compiled from the old document.
    fn adopt(&mut self, config: ProviderConfig, provenance: Provenance) -> Result<(), String> {
        // `accept` already compiled this to validate it; compiling again costs microseconds and
        // keeps this method correct for any future caller that did not come through `accept`.
        let schedule = CompiledSchedule::compile(&config.schedule).map_err(|e| e.to_string())?;

        self.config = config;
        self.provenance = provenance;
        self.schedule = Some(schedule);
        self.error = None;

        // The new config may not publish the currency the user picked — a provider can drop one.
        // Falling back to its default beats a price table full of dashes.
        let available = available_currencies(&self.config);
        if !available.contains(&self.settings.currency) {
            self.settings.currency = self.config.default_currency.clone();
            self.persist();
        }

        // The tray tooltip is derived from the same view, and its *state* may not have changed
        // even though its text has (a new `verifiedAt` alone does not move the price state).
        self.force_rerender();
        Ok(())
    }

    pub fn provider_status(&self) -> ProviderStatusView {
        ProviderStatusView {
            update_url: provider::UPDATE_URL.to_string(),
            synced: self.provenance == Provenance::Synced,
            // Taken from the view rather than re-derived, so the settings panel and the footer
            // cannot disagree about which figures are in force.
            source_label: self.view().config_source_label,
            verified_at: self.config.verified_at.clone(),
            notice: self.provider_notice.clone(),
        }
    }

    /// Forget what was last pushed to the tray, forcing the next refresh to re-render it.
    /// Needed when something other than the price state changes the tray's appearance — the
    /// language, for instance, which rewrites the tooltip text.
    pub fn force_rerender(&mut self) {
        self.rendered = Rendered::default();
    }

    pub fn settings_view(&self) -> SettingsView {
        let system_offset = system_offset_minutes();
        SettingsView {
            currency: self.settings.currency.clone(),
            available_currencies: available_currencies(&self.config),
            timezone_override_minutes: self.settings.timezone_override_minutes,
            system_offset_minutes: system_offset,
            zone_label: self.zone_label(),
            offset_label: display::render_offset(self.display_offset_minutes()),
            // "System default" first: it is the right answer for almost everyone, and the
            // fixed offsets exist only as an escape hatch.
            timezone_choices: std::iter::once(TimezoneChoice {
                minutes: None,
                label: format!(
                    "{} · {}",
                    self.locale.pick("跟随系统", "System default"),
                    display::render_offset(system_offset)
                ),
            })
            .chain(TIMEZONE_CHOICES.iter().map(|minutes| TimezoneChoice {
                minutes: Some(*minutes),
                label: display::render_offset(*minutes),
            }))
            .collect(),

            auto_check_hours: self.settings.auto_check_hours,
            auto_check_choices: AUTO_CHECK_HOURS
                .iter()
                .map(|hours| AutoCheckChoice {
                    hours: *hours,
                    label: if *hours == 0 {
                        self.locale.pick("关闭", "Off").to_string()
                    } else {
                        self.locale
                            .pick(
                                &format!("每 {hours} 小时"),
                                &format!("Every {hours} hours"),
                            )
                            .to_string()
                    },
                })
                .collect(),
            last_auto_check: self.settings.last_auto_check.clone(),

            language: self.settings.language.clone(),
            resolved_language: self.locale.tag().to_string(),
            // Language names are always shown in their own language — a picker you cannot
            // read is useless to the person who needs it.
            language_choices: std::iter::once(LanguageChoice {
                tag: None,
                label: format!(
                    "{} · {}",
                    self.locale.pick("跟随系统", "System default"),
                    system_locale().native_name()
                ),
            })
            .chain(Locale::ALL.iter().map(|locale| LanguageChoice {
                tag: Some(locale.tag().to_string()),
                label: locale.native_name().to_string(),
            }))
            .collect(),
        }
    }

    /// Re-render into the tray if anything changed. `None` means the tray is already correct.
    pub fn update_rendered(&mut self, view: &StateView) -> Option<Rendered> {
        let next = Rendered {
            state: view.state,
            tooltip: tooltip_lines(view, cfg!(target_os = "windows")).join("\n"),
        };
        if next == self.rendered {
            return None;
        }
        self.rendered = next.clone();
        Some(next)
    }

    pub fn persist(&self) {
        if let Err(e) = self.settings.save(&self.settings_dir) {
            eprintln!("{APP_NAME}: could not save settings: {e}");
        }
    }
}

fn available_currencies(config: &ProviderConfig) -> Vec<String> {
    let mut currencies: Vec<String> = config
        .models
        .iter()
        .flat_map(|m| m.prices.keys().cloned())
        .collect();
    currencies.sort();
    currencies.dedup();
    currencies
}

fn system_offset_minutes() -> i32 {
    Local::now().offset().local_minus_utc() / 60
}

/// Best-effort IANA name for the system zone, read from the `/etc/localtime` symlink.
/// Returns `None` on platforms that don't use that convention (i.e. Windows) rather than
/// pulling in a timezone database just to print a label.
fn system_zone_name() -> Option<String> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let text = target.to_string_lossy().into_owned();
    text.split("zoneinfo/")
        .nth(1)
        .filter(|zone| !zone.is_empty())
        .map(|zone| zone.to_string())
}

/// The interface language: the user's explicit choice, else the system's, else English.
fn resolve_locale(settings: &Settings) -> Locale {
    match settings.language.as_deref() {
        Some(tag) if !tag.trim().is_empty() => Locale::from_tag(tag),
        _ => system_locale(),
    }
}

/// What the system language resolves to, ignoring any override.
fn system_locale() -> Locale {
    sys_locale::get_locale()
        .map(|tag| Locale::from_tag(&tag))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimezoneChoice {
    pub minutes: Option<i32>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageChoice {
    /// `None` means "follow the system language".
    pub tag: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub currency: String,
    pub available_currencies: Vec<String>,
    pub timezone_override_minutes: Option<i32>,
    pub system_offset_minutes: i32,
    pub zone_label: String,
    pub offset_label: String,
    pub timezone_choices: Vec<TimezoneChoice>,

    pub language: Option<String>,
    /// The tag actually in force, after resolving "follow system".
    pub resolved_language: String,
    pub language_choices: Vec<LanguageChoice>,

    /// Hours between automatic price checks; `0` is off.
    pub auto_check_hours: u32,
    /// The intervals the settings panel offers, so the list lives in Rust with the default
    /// rather than being duplicated in the frontend.
    pub auto_check_choices: Vec<AutoCheckChoice>,
    /// When the last automatic check finished, RFC 3339. Shown so the user can tell "nothing has
    /// changed" apart from "it has not looked".
    pub last_auto_check: Option<String>,
}

/// One row of the automatic-check picker. `hours: 0` is the "off" row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCheckChoice {
    pub hours: u32,
    pub label: String,
}

/// Facts about the runtime that the UI needs in order to render honestly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentView {
    /// True only when the native window material was actually applied. The popover switches
    /// to an opaque background when this is false.
    pub glass: bool,
    /// `"windows"` / `"macos"` / `"linux"`. The CSS needs it for exactly one decision, and it
    /// is not cosmetic: on Windows the OS rounds the window itself, so the card must not round
    /// a second time — see the note on `popover::CARD_RADIUS`.
    pub platform: &'static str,
}

/// What the settings panel needs to describe — and refresh — the pricing data.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusView {
    /// Where to fetch a newer copy from.
    ///
    /// The frontend is handed this rather than knowing it, because the request is made from the
    /// webview and its origin has to match the CSP. Keeping the URL in Rust means there is one
    /// place to change it, and the CSP note in `provider.rs` names the other.
    pub update_url: String,
    /// True when a synced copy is in force — i.e. there is something to restore *from*.
    pub synced: bool,
    /// `Built-in · 2026-09-12` / `已同步 · 2026-09-20`. Rendered by the engine, like every other
    /// user-facing string, so it cannot drift from the footer that shows the same label.
    pub source_label: String,
    /// The provider's own date for the figures in force. The UI compares it across a sync to
    /// tell "updated to a newer copy" from "already current" — the provenance word changes on a
    /// first sync even when the figures did not, so the label alone cannot answer that.
    pub verified_at: String,
    /// Why a synced copy is *not* in force. Non-fatal: the app is running on the bundled one.
    pub notice: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_view(core: State<'_, Mutex<AppCore>>) -> Result<StateView, String> {
    Ok(lock(&core)?.view())
}

#[tauri::command]
pub fn get_settings(core: State<'_, Mutex<AppCore>>) -> Result<SettingsView, String> {
    Ok(lock(&core)?.settings_view())
}

/// Change how often the app goes and looks for new figures by itself.
///
/// `0` restores the older behaviour exactly: the app then makes a network request only when the
/// user presses the button. Anything not on the list is refused rather than clamped, so a bad
/// value cannot silently become a different interval than the one the panel shows.
#[tauri::command]
pub fn set_auto_check_hours(
    hours: u32,
    core: State<'_, Mutex<AppCore>>,
) -> Result<SettingsView, String> {
    if !AUTO_CHECK_HOURS.contains(&hours) {
        return Err(format!("{hours} is not an offered interval"));
    }
    let mut core = lock(&core)?;
    core.settings.auto_check_hours = hours;
    core.persist();
    Ok(core.settings_view())
}

/// Record that an automatic check finished, and hand the webview back.
///
/// The timestamp is written **whatever the outcome**. A machine that is offline every time the
/// timer fires would otherwise retry on every tick — which is precisely the polling this app
/// promised not to do. One attempt per interval, successful or not.
///
/// The outcome is not stored: a failed check changes nothing the user can act on, and the figures
/// on screen already carry their own provenance label (`built-in · 2026-09-12`).
/// Collect a scheduled check the tick loop asked for. Returns whether there was one, and clears
/// the flag so one request can never produce two checks.
///
/// The event alone is not a handshake. `popover::auto_check` emits `auto-check` the instant the
/// hidden window is created, but creating a webview only *starts* a page load — the listener is
/// registered when `main.js` runs, hundreds of milliseconds later. An event emitted in that gap is
/// delivered to nobody, and it fails silently, because "no listener yet" is indistinguishable from
/// "the check ran and had nothing to do".
///
/// A flag the page collects when it boots closes that window without polling, and it is strictly
/// better than the event rather than a companion to it: whichever arrives first wins, and the
/// `swap` makes the second a no-op.
#[tauri::command]
pub fn take_auto_check(core: State<'_, Mutex<AppCore>>) -> Result<bool, String> {
    let mut core = lock(&core)?;
    Ok(std::mem::replace(&mut core.auto_check_pending, false))
}

#[tauri::command]
pub fn record_auto_check(
    ok: bool,
    app: AppHandle,
    core: State<'_, Mutex<AppCore>>,
) -> Result<(), String> {
    {
        let mut core = lock(&core)?;
        core.settings.last_auto_check = Some(core.clock.now_utc().to_rfc3339());
        core.persist();
    }
    if !ok {
        eprintln!("{APP_NAME}: the scheduled price check did not go through");
    }
    crate::popover::release_after_auto_check(&app);
    Ok(())
}

#[tauri::command]
pub fn set_currency(
    currency: String,
    app: AppHandle,
    core: State<'_, Mutex<AppCore>>,
) -> Result<StateView, String> {
    {
        let mut core = lock(&core)?;
        core.settings.currency = currency;
        core.persist();
    }
    // Refresh immediately so the tray's price line and the popover agree without waiting for
    // the next boundary tick.
    crate::tick::refresh(&app);
    Ok(lock(&core)?.view())
}

#[tauri::command]
pub fn set_timezone_offset(
    minutes: Option<i32>,
    app: AppHandle,
    core: State<'_, Mutex<AppCore>>,
) -> Result<StateView, String> {
    {
        let mut core = lock(&core)?;
        core.settings.timezone_override_minutes = minutes;
        core.persist();
    }
    crate::tick::refresh(&app);
    Ok(lock(&core)?.view())
}

/// Escape hatch for the popover to dismiss itself. Clicking elsewhere is the usual path (the
/// window hides on focus loss), but Escape is the expected way out of a panel like this.
#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    crate::popover::hide(&app);
}

#[tauri::command]
pub fn get_environment(core: State<'_, Mutex<AppCore>>) -> Result<EnvironmentView, String> {
    Ok(EnvironmentView {
        glass: lock(&core)?.glass_applied,
        platform: std::env::consts::OS,
    })
}

#[tauri::command]
pub fn get_provider_status(core: State<'_, Mutex<AppCore>>) -> Result<ProviderStatusView, String> {
    Ok(lock(&core)?.provider_status())
}

/// Install a config the frontend fetched.
///
/// The JSON arrives as a string rather than as a parsed object on purpose: the frontend must not
/// be able to hand over something that only *looks* like a `ProviderConfig`. Everything — the
/// parse, the schedule compile, the sanity checks — happens here, in Rust, where it can be
/// tested without a webview.
///
/// Note the division of labour: the webview makes the request, because it already has a TLS
/// stack and the Rust side deliberately does not. This command never reaches the network.
#[tauri::command]
pub fn apply_provider_config(
    json: String,
    app: AppHandle,
    core: State<'_, Mutex<AppCore>>,
) -> Result<ProviderStatusView, String> {
    {
        let mut core = lock(&core)?;
        // Validated against what is in force *right now*, and only written if it passes — so a
        // rejected update leaves neither a changed app nor a changed file.
        let config = provider::install(&core.settings_dir, &json, &core.config)?;
        core.adopt(config, Provenance::Synced)?;
        // Whatever the last refusal was, it is no longer the situation.
        core.provider_notice = None;
    }
    crate::tick::refresh(&app);
    Ok(lock(&core)?.provider_status())
}

/// Go back to the config compiled into the binary.
#[tauri::command]
pub fn reset_provider_config(
    app: AppHandle,
    core: State<'_, Mutex<AppCore>>,
) -> Result<ProviderStatusView, String> {
    {
        let mut core = lock(&core)?;
        provider::clear(&core.settings_dir)?;
        let bundled = load_bundled().map_err(|e| e.to_string())?;
        core.adopt(bundled, Provenance::BuiltIn)?;
        core.provider_notice = None;
    }
    crate::tick::refresh(&app);
    Ok(lock(&core)?.provider_status())
}

#[tauri::command]
pub fn set_language(
    language: Option<String>,
    app: AppHandle,
    core: State<'_, Mutex<AppCore>>,
) -> Result<StateView, String> {
    {
        let mut core = lock(&core)?;
        core.settings.language = language.filter(|tag| !tag.trim().is_empty());
        core.locale = resolve_locale(&core.settings);
        core.persist();
        // The tray tooltip is in the old language until it is re-rendered, and its *state* has
        // not changed — so force the comparison rather than relying on it.
        core.force_rerender();
    }
    crate::tick::refresh(&app);
    Ok(lock(&core)?.view())
}

fn lock<'a>(
    core: &'a State<'_, Mutex<AppCore>>,
) -> Result<std::sync::MutexGuard<'a, AppCore>, String> {
    core.lock().map_err(|_| "application state is poisoned".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(stamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(stamp).unwrap().with_timezone(&Utc)
    }

    /// The default is what makes the feature exist at all: a fresh install checks on its own
    /// rather than waiting a day to be asked.
    #[test]
    fn a_check_that_has_never_run_is_due() {
        assert!(auto_check_due(at("2026-09-13T12:00:00Z"), None, auto_check_interval(24)));
    }

    /// Zero hours is off, and stays off — including for a check that has never run, which is the
    /// case that would otherwise fire immediately and make "off" mean "once".
    #[test]
    fn zero_hours_is_off_even_before_the_first_check() {
        assert_eq!(auto_check_interval(0), chrono::Duration::zero());
        assert!(!auto_check_due(at("2026-09-13T12:00:00Z"), None, auto_check_interval(0)));
        assert!(!auto_check_due(
            at("2026-09-13T12:00:00Z"),
            Some("2020-01-01T00:00:00Z"),
            auto_check_interval(0)
        ));
    }

    /// The boundary is inclusive, and the interval is what the setting says it is — the same
    /// discipline the peak/off-peak boundary is held to.
    #[test]
    fn the_interval_flips_exactly_at_the_reported_boundary() {
        let last = "2026-09-13T00:00:00Z";
        let interval = auto_check_interval(24);

        assert!(!auto_check_due(at("2026-09-13T23:59:59Z"), Some(last), interval));
        assert!(auto_check_due(at("2026-09-14T00:00:00Z"), Some(last), interval));
        assert!(auto_check_due(at("2026-09-14T00:00:01Z"), Some(last), interval));
    }

    /// A timestamp that cannot be parsed counts as "never ran" — a stale or hand-edited settings
    /// file must not be able to switch the feature off by being unreadable.
    #[test]
    fn an_unreadable_timestamp_counts_as_never_having_run() {
        assert!(auto_check_due(
            at("2026-09-13T12:00:00Z"),
            Some("not a timestamp"),
            auto_check_interval(24)
        ));
    }

    /// The interval set by the settings panel is the one that governs. 6 hours is due where 72
    /// is not, on the same pair of timestamps.
    #[test]
    fn the_chosen_interval_governs() {
        let last = "2026-09-13T00:00:00Z";
        let now = at("2026-09-13T07:00:00Z");

        assert!(auto_check_due(now, Some(last), auto_check_interval(6)));
        assert!(!auto_check_due(now, Some(last), auto_check_interval(24)));
        assert!(!auto_check_due(now, Some(last), auto_check_interval(72)));
    }

    /// A settings file written before this feature existed loads with the default, not with
    /// "off" — the same shape as the language field before it.
    #[test]
    fn a_settings_file_written_before_auto_check_existed_still_loads() {
        let settings: Settings =
            serde_json::from_str(r#"{"currency":"CNY","language":"zh"}"#).unwrap();

        assert_eq!(settings.auto_check_hours, 24);
        assert_eq!(settings.last_auto_check, None);
    }

    #[test]
    fn an_explicit_language_beats_the_system_one() {
        let mut settings = Settings::default();

        settings.language = Some("zh-CN".to_string());
        assert_eq!(resolve_locale(&settings), Locale::Zh);

        settings.language = Some("en-GB".to_string());
        assert_eq!(resolve_locale(&settings), Locale::En);

        settings.language = Some("klingon".to_string());
        assert_eq!(
            resolve_locale(&settings),
            Locale::En,
            "an unrecognised tag falls back to English"
        );
    }

    #[test]
    fn a_blank_or_absent_language_falls_through_to_the_system() {
        let mut settings = Settings::default();

        settings.language = None;
        assert_eq!(resolve_locale(&settings), system_locale());

        settings.language = Some("   ".to_string());
        assert_eq!(
            resolve_locale(&settings),
            system_locale(),
            "whitespace must not be mistaken for a language choice"
        );
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = Settings {
            currency: "USD".to_string(),
            timezone_override_minutes: Some(540),
            language: Some("zh".to_string()),
            auto_check_hours: 6,
            last_auto_check: Some("2026-09-13T12:00:00+00:00".to_string()),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();

        assert_eq!(back.currency, "USD");
        assert_eq!(back.timezone_override_minutes, Some(540));
        assert_eq!(back.language.as_deref(), Some("zh"));
        assert_eq!(back.auto_check_hours, 6);
        assert_eq!(back.last_auto_check.as_deref(), Some("2026-09-13T12:00:00+00:00"));
    }

    /// Guards the upgrade path: adding `language` must not invalidate a settings.json written
    /// by an older build, or upgrading would silently reset the user's currency and timezone
    /// along with it.
    #[test]
    fn a_settings_file_written_before_language_existed_still_loads() {
        let older = r#"{"currency":"USD","timezoneOverrideMinutes":540}"#;
        let settings: Settings = serde_json::from_str(older).unwrap();

        assert_eq!(settings.currency, "USD");
        assert_eq!(settings.timezone_override_minutes, Some(540));
        assert_eq!(settings.language, None, "a missing field means 'follow the system'");
    }
}
