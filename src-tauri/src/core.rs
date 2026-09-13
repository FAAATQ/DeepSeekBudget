//! Application state and the commands the popover calls.
//!
//! Everything here is a thin shell over `apibudget-schedule`. The rule this module follows:
//! **no failure may remove the tray icon.** A missing config, an unparseable schedule or a
//! bad `APIBUDGET_FAKE_NOW` all resolve to the grey Unknown dot plus an error string, never
//! to a panic or an absent icon.

use apibudget_schedule::{
    Clock, CompiledSchedule, Locale, PriceState, Provenance, ProviderConfig, StateView, ViewInput,
    build_view, clock_from_env, display, load_bundled, tooltip_lines,
};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, State};

use crate::provider;

pub const APP_NAME: &str = "API Budget";
const SETTINGS_FILE: &str = "settings.json";

/// Fixed offsets offered in the settings panel.
///
/// Deliberately a short list rather than a full zone database. V0.1 models the display zone
/// as a fixed offset from UTC, which is exact for zones without DST — including the +08:00
/// audience this app is built for — and a documented approximation elsewhere. "System
/// default" is the recommended choice and is what almost everyone should use.
const TIMEZONE_CHOICES: &[i32] = &[0, 480, 540, 420, 330, 60, -300, -480, -600];

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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            currency: String::new(),
            timezone_override_minutes: None,
            language: None,
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

        // A malformed APIBUDGET_FAKE_NOW is a developer typo, not a product failure. Warn on
        // stderr — the channel a developer is actually watching — and carry on with the real
        // clock. Routing it into `error` would pin the tray to a grey Unknown dot for end
        // users who could never have set this variable in the first place.
        let clock = match clock_from_env() {
            Ok(clock) => clock,
            Err(e) => {
                eprintln!("{APP_NAME}: ignoring the pinned clock — {e}");
                Box::new(apibudget_schedule::SystemClock)
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
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();

        assert_eq!(back.currency, "USD");
        assert_eq!(back.timezone_override_minutes, Some(540));
        assert_eq!(back.language.as_deref(), Some("zh"));
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
