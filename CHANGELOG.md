# Changelog

## v0.3.3 — 2026-09-14

A review pass over the whole codebase. Nothing new to look at — this release is about the places
where the app was quietly saying something untrue.

### Fixed

- **The panel could open on the wrong display.** Finding the screen the tray icon sits on meant
  converting a point between coordinate systems, and the two platforms disagree about which one
  the tray hands you — Windows gives physical pixels, macOS gives logical points. The conversion
  was applied unconditionally, so on any display that is not at 100% scale the point landed
  somewhere else. Single-monitor setups mostly got away with it; a mixed-DPI desk did not. The
  conversion is gone — the lookup is now done in the one unit both platforms agree on.
- **"Next change" could name a moment when nothing changes.** Windows that touch or overlap produce
  a boundary at the join, and the panel was reporting it as a transition — so the countdown ran to
  zero and the price was the same on the other side. A boundary is now only reported when the tier
  actually differs across it.
- **The settings panel offered "Restore built-in" to someone who had never synced anything**, and
  **the start-at-login row appeared as an empty pill** on platforms where it does not apply. Both
  are `hidden` attributes that were being overruled by the element's own `display` rule — the code
  read as correct, and the attribute silently did nothing. Same fix for the error line.
- **A setting that failed to save showed the new value anyway.** If writing the setting failed, the
  control kept displaying your choice as though it had taken, and the failure was swallowed. It now
  reverts to what is actually stored and says what went wrong.
- **The first click on Settings or About from the tray menu could do nothing.** The menu event and
  the panel's listener are not ordered, so the request could be raised before anything was
  listening and then never delivered.
- **Switching language did not refresh the start-at-login row**, leaving it in the previous
  language until the panel was reopened.
- **A malformed remote config could crash the engine instead of being rejected.** A timezone offset
  far outside the real range overflowed while being formatted. It is now rejected during
  validation, like every other untrusted field — the config is not applied and the app keeps
  running on the built-in schedule.
- **A config with an empty weekday list was accepted**, producing a schedule that had no peak hours
  at all — the app would have shown a permanent off-peak price. Now rejected.
- **The daily price sync could pick up the wrong numbers** from DeepSeek's pricing page. The parser
  now only reads the sentence that states the peak hours, and refuses to produce a config if it
  produces more windows than DeepSeek has.

---

## v0.3.2 — the prices keep themselves fresh (2026-09-13)

**The problem.** One-click sync solved "a price change should not need a release". It did not
solve **"nobody ever clicks it"**. This app lives in a menu bar and most people never open the
panel — so the button was there and the figures went stale anyway.

**Two paths, neither of which needs you to do anything**

- **Upstream.** The config repository now runs a scheduled job that reads DeepSeek's published
  pricing page once a day and **opens a pull request only when something actually changed** — a
  quiet day produces no PR and no notification.
- **Locally.** A new **automatic check** setting, defaulting to **24 hours**, with 6 / 12 / 24 / 72
  or off. The panel tells you when the last automatic check ran.

### ⚠️ The cost: the "only when you click" promise is now narrower

The app used to make a request *only* when you clicked. It now also makes one on an interval you
set — **a request you did not ask for at that moment, which this project previously said it would
never do.** The mitigations are real (once a day by default, stated plainly in the UI, switchable
off), but they are mitigations, not an absence of the cost.

What did **not** change: the request is still a plain `GET` for one public JSON file, nothing about
you is sent, there is no telemetry, the Rust tree still has no HTTP client, and the CSP still
allows exactly one origin.

---

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
  so an existing `com.aicoworks.apibudget.plist` is now simply never looked at again. Worth
  knowing if you had launch-at-login on before the rename: the old entry is inert, and turning
  the setting off and on again writes a fresh one.

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
  *Caveat:* this is a **fallback, not a repair** — with no usable status-item frame the panel
  anchors to the right end of the menu bar rather than under the icon you clicked. The real fix
  needs `ns_status_item()`, which is upstream.

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
