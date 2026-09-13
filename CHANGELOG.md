# Changelog

## v0.3.1 — 2026-09-13

**Renamed to DeepSeek Budget, and the two bugs that only macOS had.**

### Changed

- **The product is now called DeepSeek Budget** (was API Budget). Package names
  (`api-budget` → `deepseek-budget`, `apibudget-schedule` → `deepseekbudget-schedule`),
  `productName`, the bundle identifier (`com.aicoworks.apibudget` → `.deepseekbudget`), the binary
  name, and the environment-variable prefix for all four development facilities
  (`APIBUDGET_*` → `DEEPSEEKBUDGET_*`) moved with it. Entries above keep the names they shipped
  under.
- **The identifier change orphaned the old login item.** Both platforms key the entry by identifier,
  so an existing `com.aicoworks.apibudget.plist` is now simply never looked at again. This was
  written down as a coupling in the architecture notes long before it was ever hit; this rename was
  the first time it was hit for real.

### Fixed

- **A square outline around all four corners on macOS.** The panel was correctly rounded, but the
  window's rectangular bounds still drew *outside* the curve — invisible along the straight edges,
  visible only at the corners. `apply_liquid_glass` never passed a `content_view`, so
  `window-vibrancy`'s `move_primary_content_view` returned on its first line and skipped that
  crate's **only** `apply_corner_radius_layer` call (the one that sets `masksToBounds`). The fix
  sets `cornerRadius` + `masksToBounds` on the window's view — **clipping only, the material is
  untouched.** It costs one new direct dependency (`objc2`), at a version already in the lock file,
  so the tree gains nothing.
- **The panel opened in the middle of the screen on macOS.** Two bugs stacked. First, `tray.rect()`
  returns a **zero-height** rect on macOS 26 (`x=0 y=2100 w=82 h=0`); the old code logged one line
  and left the window wherever it was created — which is the middle of the screen, and **a menu bar
  app whose panel opens in the middle of the screen reads as an unfinished feature, not as a
  positioning failure.** Second, `monitor_from_point` compares against `CGDisplayBounds`, which is
  in **logical points**, while the tray rect is in **physical pixels** — so on a 2× display every
  coordinate was doubled. Single-monitor setups passed by luck.

### Fixed — the price sync did not work where most users are

- **The config was fetched from `raw.githubusercontent.com`, which is unreachable from mainland
  China.** Measured 2026-09-13: **5 of 5 attempts timed out** (19–20 s each), while the jsDelivr
  mirror of the same file answered **4 of 4 in ~0.3 s**. Since the audience for a DeepSeek price
  indicator is largely there, "Check for updates" was effectively dead for the people it was built
  for. `UPDATE_URL` and the CSP's `connect-src` now point at `cdn.jsdelivr.net` — still exactly one
  origin, so the "one opening" property is unchanged.
- **The mirror costs caching, and the cost is paid explicitly.** jsDelivr serves a branch ref with
  `s-maxage=43200` (12 h at the edge) and `max-age=604800` (7 d in the browser). The browser half
  was already handled — the frontend fetches with `cache: "no-store"`. The edge half is handled by
  a purge workflow in the config repository that runs on every push, so a price change is visible
  on the next click rather than the next day. Appending a query string does **not** work:
  measured, jsDelivr normalises it away and still answers `cf-cache-status: HIT`.

### Fixed — `tools/make-preview.py` had been quietly broken since v0.2.0

- The offscreen preview stubbed four `window.__TAURI__` commands and returned `null` for anything
  else. When the price-sync release added a fifth (`get_provider_status`), the preview started
  rendering an **error banner** instead of a panel — and nothing failed, because a stub that
  returns `null` is not an error until something reads a field off it. The tool built to catch
  "Rust emits a name the frontend does not read" had itself drifted the same way. It now derives
  the stub from the payload and from `provider.rs`, and **throws on an unknown command** rather
  than returning `null`.

### Verification

- **127 tests pass** (90 engine + 36 app + 1 doc) — 3 more than the previous release, all
  pure-function assertions on the fallback origin.
- **The corner fix was measured by subtracting two screenshots at the same reported position**: the
  difference region *is* a right-angled frame, 1.22% of pixels, meeting at the window's rectangle
  corners. Worth doing this way because the artifact is **background-dependent** — nearly invisible
  over a dark region, obvious over a light one — so "I can't see it" and "it's fixed" are not the
  same statement.
- **macOS launch-at-login exercised on real hardware for the first time**: `launchctl bootstrap`
  loaded it (`state = running`, `runs = 1`), `reconcile` was confirmed **not** to create an entry
  when none exists, and moving the app made the entry heal byte-for-byte.
- **`screencapture` permission came through**, so the menu bar icon and the Liquid Glass popover
  were photographed for the first time. **This retires the project's claim that Windows is the
  easier platform to verify** — that was only ever true because macOS was unphotographable.
- **Still not verified:** the portable `.exe`'s WebView2 dependency on a clean machine, and — on
  both platforms — that the OS actually runs the login entry at login. No logout or restart was
  performed. On macOS what remains is launchd's scheduling rather than this code, since
  `launchctl bootstrap` is the same operation launchd performs at login.

### Known limitation, stated plainly

- The macOS positioning fix is a **fallback, not a repair**. With no usable status-item frame, the
  panel anchors to the right end of the menu bar — it no longer drifts to the middle, but it does
  not promise to sit under the icon that was clicked. The real fix needs `ns_status_item()`, which
  is upstream.

---

## v0.3.0 — 2026-09-13

**Launch at login.**

### Added

- A toggle in the settings panel, **off by default**. Windows writes
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; macOS writes
  `~/Library/LaunchAgents/<bundle id>.plist` with `RunAtLoad`. Neither needs an installer or admin
  rights — which is the whole reason a portable build can have a login item at all.
- **Self-healing login entries.** Both platforms store an absolute path, and a portable build is
  defined by being movable — so moving it breaks the entry *silently*: it is still listed in Task
  Manager, it just never starts anything again. Every launch now runs a `reconcile`: **if the entry
  exists, rewrite it to point at the current binary.**
- `APIBUDGET_AUTOSTART=on|off`, a fourth development facility — writes or removes the login entry
  without opening the GUI and prints the state it read to stderr, so "Windows is blocking it" is
  something you **ask the app** rather than infer from a screenshot.
- A third state in the settings panel: **off, and blocked by Windows**. Reported honestly instead of
  pretending the switch is on, because the switch that overrides it lives in Task Manager.
- Windows' `StartupApproved\Run` is **read but never written**. It is keyed by value *name*, so
  deleting the `Run` value does not clear it — and writing it ourselves would overwrite a choice
  the user made in the OS.

### Changed

- `autostart.rs` is built on **three pure functions** (the Windows command string, the LaunchAgent
  plist, and the `plan` / `reconcile_action` decision), so all of it is assertable on a machine
  without macOS.
- **The official plugin is not used.** `tauri-plugin-autostart` delegates to `auto-launch 0.5.0`,
  which writes the Windows registry value **without quoting the path**. Measured: the same path run
  from a `.cmd` without quotes fails with `'C:\...\API' is not recognized as an internal or external
  command`, and **no process starts at all**. Without arguments it survives on CreateProcess's
  whole-string fallback; with arguments, the last candidate is "the path with the arguments glued
  on", which cannot work. This build quotes unconditionally — one shape is easier to reason about
  than two, and it means `reconcile` only has to compare the path.
- The dependency tree gains **one `winreg`, already present in the lock file** (pulled in by
  `embed-resource`). Neither the capabilities nor the CSP changed, so the structural claim that the
  Rust side has no network stack still holds under the new code.

### Fixed

- The `StartupApproved\Run` rule was initially copied from `auto-launch 0.5.0`, which only checks
  whether the trailing 8 bytes are zero. A real disabled entry on this machine — `03 00 00 00`
  followed by an all-zero timestamp — was **read backwards** by that rule: reported as enabled when
  it was disabled. The rule now reads the state word (`02` on / `03` off), and four real byte
  layouts from this machine became unit tests.

### Verification

- **124 tests pass** (90 engine + 33 app + 1 doc) — 11 more than the previous release, all of them
  pure-function assertions.
- On Windows: the content and quoting of the registry value, self-healing after the exe was moved,
  honest reporting when Task Manager had disabled the entry, and — by taking the registry value out
  and running it verbatim — that the command line really does start the tray icon. Two independent
  methods agreed on where the icon ended up.
- **Not verified:** that Windows actually executes the entry at login (no logout or restart was
  performed), and every runtime aspect of the macOS half — no Mac was available, so that code only
  had to compile.

---

## v0.2.0 — 2026-09-12

**Windows support, and prices that no longer need a new release.**

### Added

- **Windows tray support.** A portable `.exe`, no installer: `bundle.targets` is empty and a full
  `npx tauri build` was **verified** not to produce a `bundle/` directory. Acrylic window material
  confirmed on Windows 11. Measured **6,497,280 bytes = 6.2 MiB**.
- **Price sync.** A **Check for updates** button in the settings panel fetches the latest prices and
  schedule from a small public config repository, so a price change no longer requires an app
  release. Three lines of defence validate the download — structure, provider identity, and a
  non-decreasing `verifiedAt` — and **any single failure rejects the whole file**, leaving the
  current data untouched.
- **`APIBUDGET_REPORT_TRAY_RECT`**, a development facility that makes tray positioning scriptable
  instead of guessable.

### Fixed

Four blocking defects, every one of them measured rather than eyeballed:

- **The popover opened off-screen on Windows.** The positioning routine hard-coded "always below
  the icon" — correct for a top menu bar, wrong for a bottom taskbar, where the icon's bottom edge
  *is* the bottom of the screen. It also clamped only X and used the whole monitor rather than the
  work area. Rewritten to anchor by *available space* rather than by platform, as a pure function
  that can be tested on a machine without Windows.
- **The popover closed itself ~700 ms after opening.** Windows' foreground lock can refuse focus to
  a window shown without a user gesture, so the panel never really held focus and the next
  `Focused(false)` closed something nobody had seen. Now gated on "did it ever hold focus".
- **Clicking the tray icon could not close the popover.** On Windows the focus loss arrives
  *before* the tray event — an ordering macOS never produces — so the dismissal hid the panel and
  the click that caused it reopened it. Fixed with a 250 ms grace window.
- **The settings panel's bottom was unreachable, on both platforms.** The window is a fixed
  320×470 with `resizable(false)` and `html, body { overflow: hidden }`, which together turn
  "below the window" into **"unreachable"**. The card now scrolls.

### Changed

- **Windows gets square card corners.** `apply_acrylic` takes no radius parameter the way the two
  macOS calls do, so the rounding belongs to DWM there and the CSS must not draw a second curve on
  the same corner.

---

## v0.1.0 — 2026-09-12

First release: a menu bar indicator for DeepSeek's peak/off-peak API pricing.

- Tray icon in three states — off-peak blue, peak orange, unknown grey — using the official whale
  logo. The icon never shows a number; the shape says *who*, the colour says *what price*.
- Hover tooltip (five lines on macOS, a compressed three-line form on Windows, which caps tray
  tooltips at 127 UTF-16 characters).
- A popover with the countdown to the next change — **63 hours** on a Friday evening, which is the
  single most useful number this tool produces.
- Chinese / English, following the system language.
- Native window material via `window-vibrancy`: real Liquid Glass (`NSGlassEffectView`) on macOS 26,
  vibrancy on older macOS.
- Prices and schedule live in JSON, fully decoupled from the engine: adding a provider means adding
  a file, not touching code.

### Fixed

- A refresh-loop constant that could never take effect: `BOUNDARY_OVERSHOOT` was added *after* the
  60-second clamp, so the clamp swallowed it. Now clamped first, then overshot.
- `parse_time` accepted `"01:0"` — chrono's `%H:%M` allows single-digit fields. Now strict.
- The Chinese range separator used an en dash (`周一–周五`) instead of `至`. **The test was right
  and the code was wrong** — DeepSeek's own Chinese page writes `周一至周五`.
