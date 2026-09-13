# Changelog

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
