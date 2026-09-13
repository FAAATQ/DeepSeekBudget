//! The provider config on disk, and the machinery for replacing it.
//!
//! Two copies of the pricing data can exist, and the relationship between them is the whole
//! design:
//!
//! * The **bundled** copy is compiled into the binary. It is always present, it is what this
//!   build was tested against, and it is the floor the app cannot fall below.
//! * A **synced** copy is one the user asked for: fetched from [`UPDATE_URL`] by the popover
//!   and written to the app's config directory. It only ever replaces the bundled one after the
//!   engine has accepted it, so the worst possible outcome of a bad sync is *nothing happened*.
//!
//! The official prices and peak windows change on the provider's schedule, not on ours. Being
//! able to move them without cutting a release is the point of this module; being unable to
//! damage the app with them is the constraint it is built around.
//!
//! Nothing here can leave the app without a config to render. That is what keeps the tray icon
//! on screen — the rule stated at the top of `core`.

use apibudget_schedule::{Provenance, ProviderConfig, accept, load_bundled};
use std::collections::BTreeMap;
use std::path::Path;

/// Name of the synced copy inside the app's config directory.
pub const PROVIDER_FILE: &str = "provider.json";

/// Where a newer copy can be fetched from.
///
/// **Hard-coded on purpose.** Letting a user point this at an arbitrary URL would mean opening
/// the webview's `connect-src` to the whole web, and this config decides the app's only output
/// — the colour of the icon and every figure under it. One auditable origin is worth more than
/// the flexibility. A fork that wants a different host changes this one line.
///
/// The config lives in its own small public repository rather than inside this one, so that
/// updating a price does not require touching the app's repository at all — the two have
/// genuinely different lifetimes, since prices change and the app does not.
///
/// COUPLING: the origin here must match `connect-src` in `app.security.csp`
/// (`tauri.conf.json`). Change one without the other and the fetch is blocked by the webview,
/// silently and at runtime. Both files carry this note.
pub const UPDATE_URL: &str =
    "https://raw.githubusercontent.com/example/deepseek-budget-config/main/deepseek.json";

/// What a config load resolved to.
pub struct Loaded {
    /// The config in force. Never absent — a failure yields the placeholder, not `None`.
    pub config: ProviderConfig,
    pub provenance: Provenance,
    /// A problem with the *bundled* config. Fatal in the sense that there is nothing real to
    /// render, so the UI shows `Unknown`; the tray icon stays.
    pub error: Option<String>,
    /// A synced copy that was refused. **Not** an error state: the app is running happily on the
    /// bundled config. It is news the settings panel should carry, because ignoring the user's
    /// own file without saying why would be worse than the refusal itself.
    pub notice: Option<String>,
}

/// Read whichever config should be in force.
pub fn load(dir: &Path) -> Loaded {
    // A malformed bundled config is a build-time transcription error, not a runtime condition —
    // but it still must not take the tray down.
    let bundled = match load_bundled() {
        Ok(config) => config,
        Err(e) => {
            return Loaded {
                config: placeholder_config(),
                provenance: Provenance::BuiltIn,
                error: Some(e.to_string()),
                notice: None,
            };
        }
    };

    let raw = match read_provider_file(dir) {
        Ok(Some(raw)) => raw,
        Ok(None) => return bundled_only(bundled),
        Err(notice) => {
            return Loaded {
                notice: Some(notice),
                ..bundled_only(bundled)
            };
        }
    };

    // The synced copy is validated against the *bundled* config at startup, because that is
    // what would be in force if this file were not here. Later syncs are validated against
    // whatever is in force at that moment — see `install`.
    match accept(&raw, &bundled) {
        Ok(config) => Loaded {
            config,
            provenance: Provenance::Synced,
            error: None,
            notice: None,
        },
        Err(reason) => Loaded {
            notice: Some(reason.to_string()),
            ..bundled_only(bundled)
        },
    }
}

fn bundled_only(config: ProviderConfig) -> Loaded {
    Loaded {
        config,
        provenance: Provenance::BuiltIn,
        error: None,
        notice: None,
    }
}

/// Validate a candidate and, only if it is acceptable, put it on disk.
///
/// Order matters: every check runs *before* the write, so a rejected candidate leaves no trace.
/// Writing first and validating after would leave a bad file behind for the next launch to
/// re-reject — and would make "the update failed" and "the update is in force" indistinguishable
/// on disk.
///
/// `baseline` is the config currently in force. Passing it in rather than re-reading the bundled
/// one is what lets a genuine large price change land: the second sync of a new price compares
/// against that price, not against a figure frozen at build time.
pub fn install(dir: &Path, json: &str, baseline: &ProviderConfig) -> Result<ProviderConfig, String> {
    let config = accept(json, baseline).map_err(|e| e.to_string())?;
    save(dir, json)?;
    Ok(config)
}

fn save(dir: &Path, json: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    std::fs::write(dir.join(PROVIDER_FILE), json)
        .map_err(|e| format!("could not write {PROVIDER_FILE}: {e}"))
}

/// Drop the synced copy, putting the bundled config back in force.
///
/// The escape hatch for a synced config that is *valid but wrong* — well-formed enough to pass
/// every check, still not what the user wants. Deleting a file whose location they have no way
/// to discover is not a real option, so this is offered as one.
pub fn clear(dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(dir.join(PROVIDER_FILE)) {
        Ok(()) => Ok(()),
        // Already absent is success: the caller asked for the bundled config to be in force,
        // and it is.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not remove {PROVIDER_FILE}: {e}")),
    }
}

fn read_provider_file(dir: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(dir.join(PROVIDER_FILE)) {
        Ok(raw) => Ok(Some(raw)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("could not read {PROVIDER_FILE}: {e}")),
    }
}

/// Stand-in config, used only when the bundled JSON fails to parse, so the app can still run and
/// report the error instead of dying at launch.
pub fn placeholder_config() -> ProviderConfig {
    ProviderConfig {
        provider: "unknown".to_string(),
        display_name: crate::core::APP_NAME.to_string(),
        source_url: String::new(),
        verified_at: String::new(),
        default_currency: "USD".to_string(),
        schedule: apibudget_schedule::ScheduleConfig {
            reference_utc_offset_minutes: 0,
            reference_label: None,
            weekly: Vec::new(),
        },
        models: Vec::new(),
        notes: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A scratch directory that cleans up after itself.
    ///
    /// Hand-rolled rather than pulling in `tempfile`, because a dev-dependency still appears in
    /// `cargo tree` — and "the dependency tree has no HTTP client and no TLS stack" is a claim
    /// this project verifies *structurally*. Keeping the tree clean keeps that check readable
    /// at a glance, which is worth more here than the few lines saved.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("apibudget-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch directory");
            Scratch(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn bundled_json() -> String {
        apibudget_schedule::BUNDLED_DEEPSEEK_JSON.to_string()
    }

    #[test]
    fn with_no_synced_copy_the_bundled_config_is_in_force() {
        let scratch = Scratch::new("empty");
        let loaded = load(scratch.path());

        assert_eq!(loaded.provenance, Provenance::BuiltIn);
        assert!(loaded.error.is_none());
        assert!(loaded.notice.is_none(), "an absent file is not news");
        assert_eq!(loaded.config.provider, "deepseek");
    }

    #[test]
    fn a_valid_synced_copy_takes_over_and_survives_a_restart() {
        let scratch = Scratch::new("valid");

        let baseline = load_bundled().unwrap();
        let mut next = baseline.clone();
        next.verified_at = "2026-09-20".to_string();
        next.models[0].prices.get_mut("CNY").unwrap().output.peak = 16.0;
        let json = serde_json::to_string(&next).unwrap();

        install(scratch.path(), &json, &baseline).expect("a legal update must install");

        // Re-read from disk, exactly as the next launch would.
        let loaded = load(scratch.path());
        assert_eq!(loaded.provenance, Provenance::Synced);
        assert_eq!(loaded.config.verified_at, "2026-09-20");
        assert_eq!(loaded.config.models[0].prices["CNY"].output.peak, 16.0);
        assert!(loaded.error.is_none() && loaded.notice.is_none());
    }

    #[test]
    fn a_refused_update_writes_nothing_and_leaves_the_old_config_alone() {
        let scratch = Scratch::new("refused");
        let baseline = load_bundled().unwrap();

        // Install a good one first, so there is something to *not* clobber.
        let mut good = baseline.clone();
        good.verified_at = "2026-09-20".to_string();
        install(scratch.path(), &serde_json::to_string(&good).unwrap(), &baseline).unwrap();

        // Now offer a typo'd price. 800 is 100x the 8 in force.
        let mut typo = good.clone();
        typo.verified_at = "2026-09-25".to_string();
        typo.models[0].prices.get_mut("CNY").unwrap().output.peak = 800.0;

        let err = install(scratch.path(), &serde_json::to_string(&typo).unwrap(), &good)
            .expect_err("a 100x jump must be refused");
        assert!(err.contains("typo"), "{err}");

        // The file on disk is untouched — the refusal left no trace.
        let loaded = load(scratch.path());
        assert_eq!(loaded.config.verified_at, "2026-09-20");
        assert_eq!(loaded.config.models[0].prices["CNY"].output.peak, 8.0);
        assert!(loaded.notice.is_none(), "a failed *install* is not a load-time notice");
    }

    #[test]
    fn a_corrupt_file_on_disk_falls_back_to_bundled_and_says_so() {
        let scratch = Scratch::new("corrupt");
        std::fs::write(scratch.path().join(PROVIDER_FILE), "{ this is not json").unwrap();

        let loaded = load(scratch.path());

        // The app is fully functional — this is the difference between a notice and an error.
        assert_eq!(loaded.provenance, Provenance::BuiltIn);
        assert_eq!(loaded.config.provider, "deepseek");
        assert!(loaded.error.is_none());
        let notice = loaded.notice.expect("a rejected file must be reported");
        assert!(notice.contains("not valid JSON"), "{notice}");
    }

    /// A synced copy that is *older than the binary's own figures* is refused, and the bundled
    /// config takes over. Two things are pinned here — the rule, and a limitation worth stating:
    ///
    /// 1. The app cannot be walked backwards onto figures this build was never tested against.
    /// 2. Because exactly one synced copy is kept, refusing it also loses whatever was there
    ///    before it. At startup the only baseline available is the bundled config — there is no
    ///    history to compare against — so a hand-replaced older file costs the user the newer
    ///    one they had. They are told why and can sync again. Keeping a stack of past configs to
    ///    do better than that is not worth the extra state, and this test is where that
    ///    trade-off is written down.
    ///
    /// The *fetch* path does not have this limitation: `install` compares against the config in
    /// force, so a stale CDN response is refused without disturbing anything.
    #[test]
    fn a_copy_older_than_the_binarys_own_figures_falls_back_to_the_bundled_config() {
        let scratch = Scratch::new("stale");
        let baseline = load_bundled().unwrap();

        let mut newer = baseline.clone();
        newer.verified_at = "2026-09-20".to_string();
        install(scratch.path(), &serde_json::to_string(&newer).unwrap(), &baseline).unwrap();
        assert_eq!(load(scratch.path()).provenance, Provenance::Synced);

        // Someone replaces it with an older document, by hand or by restoring a backup.
        let mut older = baseline.clone();
        older.verified_at = "2026-09-01".to_string();
        std::fs::write(
            scratch.path().join(PROVIDER_FILE),
            serde_json::to_string(&older).unwrap(),
        )
        .unwrap();

        let loaded = load(scratch.path());
        assert_eq!(loaded.provenance, Provenance::BuiltIn);
        assert_eq!(loaded.config.verified_at, baseline.verified_at);
        assert!(loaded.error.is_none(), "a refused file is not a broken app");
        assert!(loaded.notice.unwrap().contains("backwards"));
    }

    /// The fetch path's version of the same rule, and the one that actually matters in
    /// practice: a stale CDN response must not overwrite a newer config that is already in
    /// force. `raw.githubusercontent.com` sends `max-age=300`, so this is a reachable state,
    /// not a hypothetical one.
    #[test]
    fn a_stale_fetch_cannot_overwrite_a_newer_config_already_in_force() {
        let scratch = Scratch::new("stale-fetch");
        let baseline = load_bundled().unwrap();

        let mut newer = baseline.clone();
        newer.verified_at = "2026-09-20".to_string();
        install(scratch.path(), &serde_json::to_string(&newer).unwrap(), &baseline).unwrap();
        let in_force = load(scratch.path()).config;

        let mut older = baseline.clone();
        older.verified_at = "2026-09-01".to_string();
        let err = install(
            scratch.path(),
            &serde_json::to_string(&older).unwrap(),
            &in_force,
        )
        .expect_err("a stale response must be refused");
        assert!(err.contains("backwards"), "{err}");

        assert_eq!(
            load(scratch.path()).config.verified_at,
            "2026-09-20",
            "the newer config must still be in force"
        );
    }

    #[test]
    fn clear_restores_the_bundled_config_and_is_idempotent() {
        let scratch = Scratch::new("clear");
        let baseline = load_bundled().unwrap();

        let mut next = baseline.clone();
        next.verified_at = "2026-09-20".to_string();
        install(scratch.path(), &serde_json::to_string(&next).unwrap(), &baseline).unwrap();
        assert_eq!(load(scratch.path()).provenance, Provenance::Synced);

        clear(scratch.path()).unwrap();
        assert_eq!(load(scratch.path()).provenance, Provenance::BuiltIn);

        // Calling it twice must not be an error — the user asked for the bundled config, and it
        // is in force either way.
        clear(scratch.path()).expect("clearing an absent file is success");
    }

    #[test]
    fn install_creates_the_directory_it_needs() {
        // The config directory does not exist on a first run.
        let scratch = Scratch::new("mkdir");
        let target = scratch.path().join("nested").join("deeper");
        let baseline = load_bundled().unwrap();

        install(&target, &bundled_json(), &baseline).expect("install must create its directory");
        assert_eq!(load(&target).provenance, Provenance::Synced);
    }
}
