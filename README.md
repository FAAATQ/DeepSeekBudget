# DeepSeek Budget

**English** · [简体中文](README.zh-CN.md)

> **See the price. Know when to wait.**

<img src="docs/screenshot.png" width="320" alt="The popover, showing peak pricing with the countdown to off-peak">

A menu bar (macOS) / system tray (Windows) indicator for DeepSeek's API pricing. It answers two
questions at a glance:

1. Is the API **peak** or **off-peak** right now?
2. **When does that change**, and how long until then?

The icon is the DeepSeek whale in one of three colours — **blue** off-peak, **orange** peak,
**grey** unknown. No numbers on the icon, no account, no telemetry. It turns a pricing schedule
into a colour you can read without thinking about it.

## Why it earns a place in your tray

DeepSeek's API costs **half as much off-peak**, and the off-peak windows are defined in UTC:

> Peak hours are **01:00–04:00 and 06:00–10:00 UTC, Monday through Friday**. All other hours are
> off-peak.

Two consequences do most of the work:

- **12:00–14:00 Beijing time is off-peak** — the lunch gap between the two windows.
- **The entire weekend is off-peak.** After 18:00 Friday, the next change is **63 hours** away.
  That countdown is the single most useful number this app produces, and it is not something
  anyone holds in their head.

## Install

Grab the latest build from the [**Releases page**](https://github.com/FAAATQ/DeepSeekBudget/releases/latest).

| Platform | Artifact | Notes |
|---|---|---|
| **Windows** | `deepseek-budget.exe` | Portable, **no installer** — run it and the tray icon appears. Needs WebView2, which ships with Windows 11 and current Windows 10. |
| **macOS** | `DeepSeek Budget.app` | Drag it anywhere and double-click. The build is ad-hoc signed, so the first launch needs **System Settings → Privacy & Security → Open Anyway**. |

Neither platform needs admin rights, and neither installs anything outside its own folder — which
is also why both can offer a login item (see below).

## What it does

- **A tray icon that is always right**, driven by a Rust engine on a background thread. It keeps
  changing colour while the popover is closed.
- **A tooltip** with the current state, the next change, and the current price. Windows caps tray
  tooltips at 127 characters, so Windows gets a compressed form.
- **A popover** with today's full schedule, the countdown, and the price table for every model.
- **Prices that update without a new release.** A **Check for updates** button pulls the latest
  figures and schedule from a small public config repository — so a price change by DeepSeek does
  not require this app to ship a new version. The copy bundled in the binary is the fallback, so
  the app behaves identically whether or not you ever press it.
- **Launch at login**, off by default. Neither platform needs an installer for this; the entry is
  re-pointed at the app's current location on every launch, so a portable build that gets moved
  keeps working.
- **Chinese and English**, following the system language.

### Everything fails grey, never silent

A corrupt config, a bad network response, a malformed date — every failure path renders a grey
**Unknown** icon plus a readable reason. The workspace release profile deliberately does **not**
set `panic = "abort"`, because a price indicator that vanishes is worse than one that admits it
does not know.

Remote pricing data is treated as **untrusted input**: structure, provider identity, schedule
validity, a non-decreasing date, and a 20× price-deviation limit are all checked, and **any single
failure rejects the whole file** — your current data is kept and the tray icon is untouched.

### Network

There is exactly one outbound request this app can make, and only when you click **Check for
updates**. No polling, no telemetry, no startup call. The Rust dependency tree contains no HTTP
client and no TLS stack at all — the single `fetch` runs in the webview, which already has one,
against a Content-Security-Policy that allows exactly one origin.

## Development

Prerequisites: Rust (stable ≥ 1.90), Node (only for the Tauri CLI), and on macOS the Xcode Command
Line Tools (**not** full Xcode).

```bash
npm install                                 # only to get @tauri-apps/cli
npm run dev                                 # launches; the icon appears in the menu bar

cargo test --workspace                      # 127 tests
cargo test -p deepseekbudget-schedule       # engine only, ~0.6 s

npx tauri build --bundles app               # macOS .app
cargo build --release -p deepseek-budget    # Windows portable exe
```

The engine (`crates/schedule`) has **zero Tauri dependencies** and never reads the system clock —
"now" is always a parameter. That is what makes every boundary testable headlessly.

### Time travel

Peak/off-peak boundaries cannot be verified by waiting; the next one may be 63 hours away. So
"now" can be pinned:

```bash
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T02:00:00Z npm run dev   # Monday 10:00 Beijing → peak
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T03:59:59Z npm run dev   # one second of peak left
DEEPSEEKBUDGET_FAKE_NOW=2026-09-12T02:00:00Z npm run dev   # Saturday → off-peak all day
```

It must be RFC 3339. A typo warns on stderr and falls back to the real clock rather than pinning
the icon grey forever.

### Looking at the UI without a screen recording permission

The popover is rendered offscreen from the **actual** `ui/index.html`, `ui/styles.css` and
`ui/main.js` — read at generation time, never copied:

```bash
cargo run -q -p deepseekbudget-schedule --example view -- --json 2026-09-14T02:00:00Z 480 CNY en \
  > /tmp/peak.json
python3 tools/make-preview.py /tmp/peak.json /tmp/peak.html
```

## Known limitations

1. **Opening the popover takes focus** from whatever you were using.
2. **Timezones are modelled as fixed UTC offsets.** Exact where there is no DST (including the
   UTC+8 audience this is built for); a documented approximation elsewhere. The peak/off-peak
   *judgement* is always made in UTC, so the state is always correct — only the displayed local
   time can be an hour off.
3. **"The OS runs the login item at login" is inferred, not observed.** The entry's content, its
   self-healing, and (on Windows) that the command line really does start the tray icon were all
   checked. Neither machine was ever logged out or restarted.
4. **A moved portable build is healed on the *next* launch.** The login that happens between the
   move and that launch is lost, silently.
5. **The macOS positioning fallback is a fallback.** When the status item's frame is unavailable,
   the panel anchors to the right end of the menu bar rather than under the icon you clicked.
6. **No single-instance guard.** Two launches means two tray icons.
7. **Config-error messages are English only.**

## Documentation

Code can be read. **Why it was built this way, and what each platform actually does** cannot.

| Document | Contents |
|---|---|
| [`docs/domain-pricing.md`](docs/domain-pricing.md) | The DeepSeek peak/off-peak rules this app encodes, how timezones are modelled, and how far the official API actually lets you measure your own spend |
| [`docs/windows.md`](docs/windows.md) | What the Windows port cost: popover positioning, the foreground lock, why acrylic forces square corners, the 127-character tooltip limit, and three traps in automating Windows UI verification |
| [`docs/macos.md`](docs/macos.md) | A `tray.rect()` that returns a zero-height rectangle, a monitor lookup that takes logical points while being handed physical pixels, who owns the corner radius, and a platform-verifiability claim that had to be retracted |
| [`docs/icon-pipeline.md`](docs/icon-pipeline.md) | How the tray icon is produced, why the canvas is 49×36 rather than square, and a measured comparison against shipping menu bar apps |

## Licence

MIT. See [`LICENSE`](LICENSE). The DeepSeek whale logo belongs to DeepSeek; this project is
unaffiliated with them.
