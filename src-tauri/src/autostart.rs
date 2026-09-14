//! Start at login — the login item, and the one thing that makes it work for a portable app.
//!
//! Both platforms implement this the same way in spirit: Windows writes a value under
//! `HKCU\...\CurrentVersion\Run`, macOS writes a LaunchAgent plist under
//! `~/Library/LaunchAgents/`. Neither needs an installer, administrator rights, or a system
//! directory — which is why a green/portable build can have one at all.
//!
//! What both of them record is an **absolute path**, and that is the whole problem this module
//! is built around. This app's distribution model is "unzip it wherever you like" / "drag the
//! .app anywhere", so the path changes; when it does, the login item silently stops working and
//! **neither platform tells anyone**. The stale entry is still listed in Task Manager, the stale
//! plist still exists. So writing the entry is only half the feature — [`reconcile`] runs on
//! every launch and keeps an existing entry pointing at the copy that is running now.
//!
//! Two rules the rest of this file follows:
//!
//! * **The OS is the only source of truth for what the UI shows.** [`set_enabled`] writes and
//!   then re-reads; it never echoes back what it was asked for. A write that failed still leaves
//!   the panel showing the real state, with the reason next to it.
//! * **Nothing here may fail the app.** [`reconcile`] returns `()`, prints to stderr and gives
//!   up — the tray icon must survive any failure, including this one.

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
// Only the macOS half reads the identifier off the bundle (`app.config()`); on Windows the
// value name is a plain constant, so the trait would be an unused import there.
#[cfg(target_os = "macos")]
use tauri::Manager;

use crate::core::APP_NAME;

/// Development affordance, same spirit as `DEEPSEEKBUDGET_FAKE_NOW` and `DEEPSEEKBUDGET_OPEN_POPOVER`:
/// apply a login-item setting at launch, so the whole path can be driven — and verified — by a
/// script instead of by a human clicking the settings panel. Accepts `on`/`off` (also
/// `1`/`0`, `true`/`false`). Anything else is a typo, and a typo is warned about rather than
/// obeyed: same rule as a malformed fake clock, because the only person who can set this
/// variable is a developer.
pub const AUTOSTART_ENV: &str = "DEEPSEEKBUDGET_AUTOSTART";

/// The name the login item is filed under on Windows. Task Manager's Startup tab displays this
/// string verbatim, so it is the human-readable app name rather than the bundle identifier.
///
/// `HKCU\...\Run` is a shared namespace, and this is also the name an MSI/NSIS installer would
/// want for itself. That is fine **only** while `tauri.windows.conf.json` keeps
/// `bundle.targets: []` — this app ships as a bare exe with no installer, so there is nobody to
/// collide with. If an installer is ever added, this value has to move with it.
pub const WINDOWS_VALUE_NAME: &str = APP_NAME;

// ---------------------------------------------------------------------------
// Pure: the strings we hand to the operating system
//
// These are the part of this feature that *can* be tested, and they are where the mistakes
// live, so they are deliberately free of any platform code — the same reason `panel_origin` is
// a pure function in `popover.rs`. Nothing below calls the OS.
// ---------------------------------------------------------------------------

/// The exact string Windows will run at login for a binary at `exe`.
///
/// **The quotes are the point.** A portable exe gets unzipped wherever the user likes, and that
/// path can contain spaces. An unquoted command line is resolved by trying successively longer
/// prefixes and only then the whole string, so it *usually* works — until an argument is
/// appended, at which point the last candidate is the path with the argument glued on and
/// nothing resolves. Until then it is also one stray `C:\...\API.exe` away from running the
/// wrong program. The official `tauri-plugin-autostart` writes this value unquoted
/// (`auto-launch 0.5.0`, `src/windows.rs`: `format!("{} {}", app_path, args.join(" "))`); this
/// deliberately does not.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_run_value(exe: &Path, args: &[&str]) -> String {
    let mut value = quote_path(&exe.to_string_lossy());
    for arg in args {
        value.push(' ');
        value.push_str(&quote_arg(arg));
    }
    value
}

/// The path half of a command line — quoted **unconditionally**, not only when it happens to
/// contain a space.
///
/// One shape rather than two: the recorded value stays a pure function of the path, so a copy of
/// the app that moved from `C:\My Apps\` to `C:\tools\` differs in exactly one way, and
/// [`reconcile`] never has to reason about whether the *format* changed. It is also the form
/// Windows itself recommends for a command line.
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_path(path: &str) -> String {
    format!("\"{}\"", path.replace('"', "\\\""))
}

/// An argument — quoted only when leaving it bare would change how the command line is split.
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_arg(argument: &str) -> String {
    if argument.is_empty() || argument.contains([' ', '\t', '"']) {
        format!("\"{}\"", argument.replace('"', "\\\""))
    } else {
        argument.to_string()
    }
}

/// The LaunchAgent plist macOS will load at login.
///
/// `ProgramArguments` holds the executable *inside* the bundle (`…/DeepSeek Budget.app/Contents/
/// MacOS/deepseek-budget`), not the `.app` directory — launchd execs the first argument, and a
/// directory is not executable. This is also what the official plugin ends up writing.
///
/// Everything interpolated is XML-escaped: a path is user data, and a user whose account is
/// named `A&B` would otherwise get a plist that launchd refuses to parse at all.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn launch_agent_plist(label: &str, exe: &Path, args: &[&str]) -> String {
    let mut arguments = format!("    <string>{}</string>", xml_escape(&exe.to_string_lossy()));
    for arg in args {
        arguments.push_str(&format!("\n    <string>{}</string>", xml_escape(arg)));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        label = xml_escape(label),
        arguments = arguments
    )
}

fn xml_escape(text: &str) -> String {
    // `&` first, or the ampersands introduced below would be escaped a second time.
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Where the LaunchAgent lives for a user whose home directory is `home`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn launch_agent_path(home: &Path, label: &str) -> PathBuf {
    home.join("Library").join("LaunchAgents").join(format!("{label}.plist"))
}

// ---------------------------------------------------------------------------
// Pure: what to do about it
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The OS already holds exactly what we would write.
    Nothing,
    Write,
    Remove,
}

/// Whether a `StartupApproved\Run` blob means "Windows has been told not to run this".
///
/// Pure, so the shapes it was built from can be pinned by tests on any machine — which matters
/// here, because the one implementation that was available to copy gets a real case backwards.
///
/// The blob is 12 bytes: a 4-byte state word followed by an 8-byte FILETIME. **The state word
/// is the signal:** `02 00 00 00` enabled, `03 00 00 00` disabled. `0x06` (enabled by policy)
/// also runs, so only `03` blocks. The timestamp is *not* a second signal —
/// `auto-launch 0.5.0` decides on "are the trailing eight bytes all zero", and a genuinely
/// disabled entry can be written as `03 00 00 00 00 00 00 00 00 00 00 00` — a zero timestamp —
/// which that rule reads as *enabled*.
///
/// The four shapes below are copied out of a real `StartupApproved\Run` key: two enabled
/// entries, one disabled with a timestamp, and one disabled without.
pub fn approval_says_blocked(blob: &[u8]) -> bool {
    blob.len() >= 4 && blob[0] == 0x03
}

/// What [`set_enabled`] should do, given what the OS records and what the user is asking for.
///
/// `desired` is `None` when the login item should not exist, which is why it is an `Option`
/// rather than an empty string: "turn it off" and "write an empty value" are different
/// instructions and a `&str` cannot tell them apart.
///
/// Note the direction the comparison runs in: the recorded value is only ever compared for
/// **equality** against what we would write now. It is deliberately never parsed. Anything that
/// is not byte-identical — a value written by another tool, an older format, a hand edit, a
/// `REG_EXPAND_SZ` — lands on `Write`, which is the action that converges. A parser would have
/// to have an opinion about each of those, and being wrong about one of them means an entry
/// that is either rewritten forever or never repaired.
pub fn plan(recorded: Option<&str>, desired: Option<&str>) -> Action {
    match desired {
        None => {
            if recorded.is_some() {
                Action::Remove
            } else {
                Action::Nothing
            }
        }
        Some(desired) => {
            if recorded == Some(desired) {
                Action::Nothing
            } else {
                Action::Write
            }
        }
    }
}

/// What [`reconcile`] should do at startup.
///
/// Deliberately not [`plan`]: the rule differs in one direction, and that direction is the
/// dangerous one. `plan` acts on what the user asked for, so `(None, Some(_))` means "they just
/// turned it on". `reconcile` runs on every launch without being asked, so for it
/// `(None, Some(_))` must mean **"they never turned it on — leave the machine alone"**. A
/// reconcile that wrote in that case would register every user's app at login the second time
/// they opened it.
pub fn reconcile_action(recorded: Option<&str>, desired: &str) -> Action {
    match recorded {
        None => Action::Nothing,
        Some(recorded) if recorded == desired => Action::Nothing,
        Some(_) => Action::Write,
    }
}

/// What the settings panel needs in order to describe — honestly — the state of the login item.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartView {
    /// `None` means "could not be determined", which is **not** the same as `false`. The panel
    /// shows the same grey Unknown it shows for a price it cannot read, rather than claiming the
    /// entry is off when the registry could not be opened.
    pub enabled: Option<bool>,
    /// The login item exists but the operating system has been told not to run it — Windows
    /// records this when the user switches the item off in Task Manager. Distinct from `enabled:
    /// false`, because the fix is in Task Manager and not in this panel.
    pub blocked: bool,
    pub supported: bool,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Platform glue
//
// Every function returns a `Result` rather than swallowing failures: a write that quietly did
// nothing would leave the panel reporting a state the OS is not in, which is the one thing this
// module is written to avoid.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use super::WINDOWS_VALUE_NAME;
    use tauri::AppHandle;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

    const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";

    /// Where Windows records that the user switched a startup item off in Task Manager.
    ///
    /// Keyed by the **value name**, which is why deleting the value under `Run` does not clear
    /// it — the record survives, and the next value written under that name is refused too.
    const APPROVED_KEY: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    /// `Ok(None)` means the value is absent, i.e. the login item is off. `Err` means it could not
    /// be *read*, which is not the same fact and must never be rendered as "off".
    pub fn recorded(_app: &AppHandle) -> Result<Option<String>, String> {
        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_READ)
            .map_err(|e| format!("could not open the startup registry key: {e}"))?;

        match run.get_value::<String, _>(WINDOWS_VALUE_NAME) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("could not read the startup entry: {e}")),
        }
    }

    /// Whether Windows has been told not to run this entry.
    ///
    /// **Read-only, deliberately.** Task Manager writes this blob when the user switches the
    /// item off; writing it ourselves would overrule a choice the user made in the OS, and the
    /// format is undocumented — which is also why an absent key, or an absent value, means
    /// *enabled*: we only ever report a block Windows actually recorded.
    pub fn blocked(_app: &AppHandle) -> bool {
        let Ok(key) =
            RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(APPROVED_KEY, KEY_READ)
        else {
            return false;
        };
        let Ok(raw) = key.get_raw_value(WINDOWS_VALUE_NAME) else {
            return false;
        };
        super::approval_says_blocked(&raw.bytes)
    }

    pub fn write(_app: &AppHandle, value: &str) -> Result<(), String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("could not open the startup registry key: {e}"))?
            .set_value(WINDOWS_VALUE_NAME, &value.to_string())
            .map_err(|e| format!("could not write the startup entry: {e}"))
    }

    pub fn remove(_app: &AppHandle) -> Result<(), String> {
        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("could not open the startup registry key: {e}"))?;

        match run.delete_value(WINDOWS_VALUE_NAME) {
            Ok(()) => Ok(()),
            // Already gone is the state we were aiming for.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("could not remove the startup entry: {e}")),
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::launch_agent_path;
    use std::path::{Path, PathBuf};
    use tauri::{AppHandle, Manager};

    /// The plist this app owns, at `~/Library/LaunchAgents/<bundle identifier>.plist`.
    ///
    /// The file name is derived from the bundle identifier rather than from a constant here, so
    /// there is nothing to keep in sync with `tauri.conf.json` — the identifier is already the
    /// thing macOS uses to tell one app from another. The cost is the reverse: **changing the
    /// identifier orphans the old plist**, which would keep launching the app forever with
    /// nothing to attribute it to. Renaming the bundle therefore means deleting the old file.
    fn file(app: &AppHandle) -> Result<PathBuf, String> {
        let home = app
            .path()
            .home_dir()
            .map_err(|e| format!("could not find the home directory: {e}"))?;
        Ok(launch_agent_path(&home, &app.config().identifier))
    }

    pub fn recorded(app: &AppHandle) -> Result<Option<String>, String> {
        let file = file(app)?;
        match std::fs::read_to_string(&file) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("could not read {}: {e}", file.display())),
        }
    }

    /// macOS has no equivalent of Windows' Task Manager startup approval: a login item is on if
    /// the plist is there and the user has not removed it in System Settings.
    pub fn blocked(_app: &AppHandle) -> bool {
        false
    }

    pub fn write(app: &AppHandle, value: &str) -> Result<(), String> {
        let file = file(app)?;
        let directory: &Path = file
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", file.display()))?;
        // `~/Library/LaunchAgents` does not exist on a fresh account, and `File::create` would
        // fail rather than make it — a silent failure the first time anyone switches this on.
        std::fs::create_dir_all(directory)
            .map_err(|e| format!("could not create {}: {e}", directory.display()))?;

        // Write-then-rename: a half-written plist is worse than no plist, because the state the
        // panel reports — and the state launchd acts on — is "this file exists".
        let temporary = file.with_extension("plist.writing");
        std::fs::write(&temporary, value)
            .map_err(|e| format!("could not write {}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, &file)
            .map_err(|e| format!("could not install {}: {e}", file.display()))
    }

    pub fn remove(app: &AppHandle) -> Result<(), String> {
        let file = file(app)?;
        match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("could not remove {}: {e}", file.display())),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use tauri::AppHandle;

    pub fn recorded(_app: &AppHandle) -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn blocked(_app: &AppHandle) -> bool {
        false
    }

    pub fn write(_app: &AppHandle, _value: &str) -> Result<(), String> {
        Err("start at login is not implemented on this platform".to_string())
    }

    pub fn remove(_app: &AppHandle) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The feature
// ---------------------------------------------------------------------------

pub fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// The one place this binary's own path is resolved.
///
/// It feeds **both** what we write and what we compare against, and that is load-bearing: a
/// second implementation on either side (one canonicalised, the other raw, one following a
/// symlink and the other not) would disagree with itself and rewrite the entry on every single
/// launch. Anything that changes here changes both halves at once.
fn exe_path() -> Option<PathBuf> {
    match std::env::current_exe() {
        Ok(path) => Some(path),
        Err(e) => {
            eprintln!("{APP_NAME}: could not resolve this binary's own path — {e}");
            None
        }
    }
}

/// What the login item should contain for the copy that is running right now.
fn desired(app: &AppHandle) -> Option<String> {
    let exe = exe_path()?;

    #[cfg(windows)]
    {
        let _ = app;
        Some(windows_run_value(&exe, &[]))
    }

    #[cfg(target_os = "macos")]
    {
        // The identifier is read from the bundle rather than written down here, so the plist
        // name cannot drift from what macOS thinks this app is.
        Some(launch_agent_plist(&app.config().identifier, &exe, &[]))
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (app, exe);
        None
    }
}

/// Read the state of the login item out of the operating system. Never fails: a failure to read
/// comes back as `enabled: None` plus the reason, which is a different thing from "off".
pub fn view(app: &AppHandle) -> AutostartView {
    if !supported() {
        return AutostartView { enabled: None, blocked: false, supported: false, error: None };
    }

    match platform::recorded(app) {
        Ok(recorded) => {
            let exists = recorded.is_some();
            let blocked = exists && platform::blocked(app);
            AutostartView { enabled: Some(exists && !blocked), blocked, supported: true, error: None }
        }
        Err(e) => {
            AutostartView { enabled: None, blocked: false, supported: true, error: Some(e) }
        }
    }
}

/// Turn the login item on or off, then report what the OS actually ended up holding.
///
/// The return value is a fresh [`view`], never an echo of `enabled`: a write that failed, or one
/// that Windows refuses to honour, has to show up as the state the user will actually get. The
/// failure travels in `error` on that view rather than as a command error, so the panel can show
/// the real state *and* the reason at the same time.
pub fn set_enabled(app: &AppHandle, enabled: bool) -> AutostartView {
    if !supported() {
        return view(app);
    }

    let mut error = None;

    if enabled {
        match desired(app) {
            None => {
                error = Some("could not work out where this application is installed".to_string())
            }
            Some(value) => {
                // Skip the write when the OS already holds exactly this string. On macOS a
                // rewritten plist is a fresh registration with the background-task manager, so
                // clicking a switch that is already on should not touch the disk at all.
                let action = match platform::recorded(app) {
                    Ok(recorded) => plan(recorded.as_deref(), Some(&value)),
                    // The read failed, so there is nothing to compare against. Take the user at
                    // their word: a redundant write costs nothing, while a skipped one is a
                    // switch that does nothing at all.
                    Err(_) => Action::Write,
                };
                if action == Action::Write {
                    if let Err(e) = platform::write(app, &value) {
                        error = Some(e);
                    }
                }
            }
        }
    } else if let Err(e) = platform::remove(app) {
        // Removing is idempotent, so there is no read to make first.
        error = Some(e);
    }

    // The read-back is the whole point: whatever the write did or failed to do, this is the
    // state the user is actually in.
    let mut state = view(app);
    if state.error.is_none() {
        state.error = error;
    }
    state
}

/// Keep an existing login item pointing at the copy of the app that is running now.
///
/// This is what makes the feature survive the way this app is actually distributed: a portable
/// exe or a `.app` gets moved, and the absolute path the login item recorded goes stale — with
/// no error, no notification, and nothing in either OS's UI that says so. Running on every
/// launch, this notices and rewrites it.
///
/// Two invariants, both tested:
///
/// * **It never creates a login item.** `recorded == None` means the user has not asked for one,
///   and a reconcile that wrote in that case would register the app at login for everyone who
///   ever opened it. Only [`set_enabled`] — a user gesture — may create one.
/// * **It never touches the OS when nothing changed**, so a correctly configured machine does no
///   writes at all on boot.
///
/// Infallible on purpose. It runs inside Tauri's `setup`, where an error or a panic means the
/// process exits before the tray icon is ever built — and a price indicator that vanishes is
/// worse than one whose login item did not get refreshed.
pub fn reconcile(app: &AppHandle) {
    if !supported() {
        return;
    }

    let recorded = match platform::recorded(app) {
        Ok(recorded) => recorded,
        Err(e) => {
            eprintln!("{APP_NAME}: could not check the login item — {e}");
            return;
        }
    };

    let Some(wanted) = desired(app) else { return };

    match reconcile_action(recorded.as_deref(), &wanted) {
        Action::Write => {
            if let Err(e) = platform::write(app, &wanted) {
                eprintln!("{APP_NAME}: could not update the login item — {e}");
            }
        }
        Action::Nothing | Action::Remove => {}
    }
}

/// Apply [`AUTOSTART_ENV`] if it is set, and **print what the OS ended up holding**.
/// Development affordance — see the constant.
///
/// The report is the point, not a leftover. The state this feature cares about — in particular
/// "the entry is there but Windows has been told not to run it" — is not something the registry
/// alone answers, and the alternative is reading it off a screenshot of the settings panel.
/// Asking the app costs one line and is exact, which is the same reason
/// `DEEPSEEKBUDGET_REPORT_TRAY_RECT` exists.
pub fn apply_env(app: &AppHandle) {
    let Ok(raw) = std::env::var(AUTOSTART_ENV) else { return };

    let wanted = match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "1" | "true" => true,
        "off" | "0" | "false" => false,
        other => {
            eprintln!("{APP_NAME}: ignoring {AUTOSTART_ENV}=\"{other}\" — expected on or off");
            return;
        }
    };

    let state = set_enabled(app, wanted);
    eprintln!(
        "{APP_NAME}: {AUTOSTART_ENV}={} — enabled={:?} blocked={} error={:?}",
        if wanted { "on" } else { "off" },
        state.enabled,
        state.blocked,
        state.error
    );
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> AutostartView {
    view(&app)
}

#[tauri::command]
pub fn set_autostart(enabled: bool, app: AppHandle) -> AutostartView {
    set_enabled(&app, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing in this module's tests may call `platform::*`. A test that did would register the
    // *test binary* as a login item on whoever ran `cargo test` — a side effect on the
    // maintainer's machine, which is exactly why `provider.rs` hand-rolls its scratch directory
    // instead of taking a temp-dir dev-dependency. The platform glue is therefore untested by
    // construction, and the pure functions below carry the weight.

    /// The bug the quoting exists for. A portable exe is unpacked wherever the user likes, and
    /// every one of those places can have a space in it.
    #[test]
    fn a_path_with_spaces_is_quoted() {
        let value = windows_run_value(Path::new(r"C:\Users\me\My Apps\deepseek-budget.exe"), &[]);
        assert_eq!(value, r#""C:\Users\me\My Apps\deepseek-budget.exe""#);
    }

    /// A path without spaces is quoted too, so that the recorded string is a function of the
    /// path alone. If it were not, moving the app from a path with a space to one without would
    /// change the format as well as the path — and `reconcile` would have two reasons to write.
    #[test]
    fn a_plain_path_is_quoted_too() {
        let value = windows_run_value(Path::new(r"C:\tools\deepseek-budget.exe"), &[]);
        assert_eq!(value, r#""C:\tools\deepseek-budget.exe""#);
    }

    /// The case the upstream plugin gets wrong: with an unquoted path and an argument, the last
    /// thing Windows tries is the path with the argument glued onto it, which is not a file.
    #[test]
    fn arguments_are_quoted_as_well() {
        let value = windows_run_value(Path::new(r"C:\My Apps\deepseek-budget.exe"), &["--quiet", "--a b"]);
        assert_eq!(value, r#""C:\My Apps\deepseek-budget.exe" --quiet "--a b""#);
    }

    #[test]
    fn the_plist_runs_at_load_and_names_the_binary_inside_the_bundle() {
        let plist = launch_agent_plist(
            "com.aicoworks.deepseekbudget",
            Path::new("/Applications/DeepSeek Budget.app/Contents/MacOS/deepseek-budget"),
            &[],
        );

        assert!(plist.contains("<key>Label</key>\n  <string>com.aicoworks.deepseekbudget</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains(
            "<string>/Applications/DeepSeek Budget.app/Contents/MacOS/deepseek-budget</string>"
        ));
        // The `.app` directory is not executable; pointing launchd at it produces a plist that
        // silently never runs.
        assert!(!plist.contains("<string>/Applications/DeepSeek Budget.app</string>"));
    }

    /// A path is user data. A home directory containing `&` or `<` would otherwise produce a
    /// plist that `launchd` cannot parse — and the failure would look like "the toggle did
    /// nothing", on a machine nobody else can reproduce.
    #[test]
    fn the_plist_escapes_xml_in_the_path() {
        let plist = launch_agent_plist(
            "com.aicoworks.deepseekbudget",
            Path::new("/Users/A&B <home>/DeepSeek Budget.app/Contents/MacOS/deepseek-budget"),
            &[],
        );

        assert!(plist.contains("A&amp;B &lt;home&gt;"), "the raw characters must not survive");
        assert!(!plist.contains("A&B"), "an unescaped ampersand makes the plist unparseable");
    }

    #[test]
    fn the_launch_agent_lives_where_macos_looks_for_it() {
        let path = launch_agent_path(Path::new("/Users/me"), "com.aicoworks.deepseekbudget");
        assert_eq!(
            path,
            Path::new("/Users/me/Library/LaunchAgents/com.aicoworks.deepseekbudget.plist")
        );
    }

    /// `desired == None` must mean "remove it", not "write an empty value" — the two are
    /// different instructions and only one of them turns the login item off.
    #[test]
    fn turning_it_off_removes_rather_than_blanks() {
        assert_eq!(plan(Some("anything"), None), Action::Remove);
        assert_eq!(plan(None, None), Action::Nothing);
        assert_eq!(plan(None, Some("value")), Action::Write);
        assert_eq!(plan(Some("old"), Some("value")), Action::Write);
    }

    /// The convergence property. Without it, a machine whose entry is already correct would
    /// still get a registry write on every single boot.
    #[test]
    fn an_entry_that_already_matches_is_left_alone() {
        let value = windows_run_value(Path::new(r"C:\My Apps\deepseek-budget.exe"), &[]);
        assert_eq!(plan(Some(&value), Some(&value)), Action::Nothing);
    }

    /// The one thing reconcile must never do: register an app at login because it was opened.
    #[test]
    fn reconcile_never_creates_a_login_item() {
        assert_eq!(reconcile_action(None, "anything"), Action::Nothing);
    }

    /// Four blobs copied out of a real `HKCU\…\StartupApproved\Run` key, so the rule is pinned
    /// to what Windows actually writes rather than to a description of it.
    ///
    /// The last one is the case that matters: disabled, with a zero timestamp — exactly what the
    /// "are the trailing eight bytes zero" rule — the only implementation available to copy —
    /// reads as *enabled*. A switch that says "on" while Windows will not launch the app is the
    /// failure this whole module exists to avoid.
    #[test]
    fn the_approval_blob_is_read_from_its_state_word_not_its_timestamp() {
        let enabled = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let disabled_with_time =
            [0x03, 0, 0, 0, 0xC1, 0x3E, 0x9A, 0xF7, 0xC8, 0x0B, 0xDD, 0x01];
        let disabled_without_time = [0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        assert!(!approval_says_blocked(&enabled), "a running entry");
        assert!(approval_says_blocked(&disabled_with_time), "disabled, timestamp present");
        assert!(
            approval_says_blocked(&disabled_without_time),
            "disabled, zero timestamp — the case the naive rule gets backwards"
        );

        // An absent or truncated blob is not a block: we only report what Windows recorded.
        assert!(!approval_says_blocked(&[]));
        assert!(!approval_says_blocked(&[0x03, 0x00]));
    }

    /// ...but it does repair one that exists. This is the portable-app case: the entry was made
    /// where the app used to live, and the app has since been moved.
    #[test]
    fn reconcile_repairs_an_entry_that_points_somewhere_else() {
        assert_eq!(reconcile_action(Some("old path"), "new path"), Action::Write);
        assert_eq!(reconcile_action(Some("same"), "same"), Action::Nothing);
    }
}
