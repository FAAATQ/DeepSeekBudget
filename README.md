# DeepSeek Budget

> **See the price. Know when to wait.**
> One glance tells you whether DeepSeek's API is worth using right now.

A tiny menu bar (macOS) / system tray (Windows) indicator that answers two questions about
DeepSeek's peak/off-peak API pricing:

1. Is the API **peak** or **off-peak** right now?
2. **When does that change**, and how long until then?

No usage tracking, no account, no telemetry. It does exactly one thing: turn DeepSeek's pricing
schedule into **a colour you can read at a glance** — the official whale logo, blue for
off-peak, orange for peak.

---

## The pricing rule it encodes

Source: <https://api-docs.deepseek.com/quick_start/pricing>, verified **2026-09-12**.

> Peak hours are **01:00–04:00 and 06:00–10:00 UTC, Monday through Friday** (all other hours
> are off-peak).

Two consequences worth the whole app:

- **12:00–14:00 Beijing time is off-peak** (the lunch gap between the two windows).
- **The entire weekend is off-peak** — after 18:00 Friday, the next change is **63 hours** away.
  That is the single most useful fact this tool surfaces.

---

## Status

| | |
|---|---|
| Tests | ✅ **127 passing** (90 engine + 36 app + 1 doc), including bilingual assertions |
| macOS menu bar | ✅ Built, run, and **photographed** — both the menu bar icon and the Liquid Glass popover were captured with `screencapture` (2026-09-13). Two macOS-only defects found and fixed (see below) |
| Windows tray | ✅ Built and run on Windows 11. Four blocking defects found and fixed (see below); acrylic material confirmed |
| Languages | ✅ Chinese / English, follows the system language by default |
| Window material | ✅ Native Liquid Glass (`NSGlassEffectView`) on macOS 26; acrylic on Windows |
| Launch at login | ✅ A toggle in the settings panel (off by default). Windows writes `HKCU\…\Run`, macOS writes a LaunchAgent — **no installer, no admin rights**. A portable build that gets moved re-points the entry on its next launch. On macOS this was exercised for real: `launchctl bootstrap` loaded it (`state = running`, `runs = 1`) and moving the app made the entry heal **byte-for-byte**. ⚠️ **"the OS runs it at login" is still inference** — see the limitations below |
| Artifact size | macOS **4.5 MB** single `.app`; Windows a **portable `.exe`**, **measured 6,514,176 bytes = 6.2 MiB**, no installer |
| Network | ⚠️ **Only one opening, and only when you click it.** The Rust dependency tree contains no HTTP client and no TLS stack; the one `fetch` runs in the webview, gated by a CSP that allows exactly one origin. |

### Four blocking defects fixed during the Windows port

All measured on real hardware, not eyeballed:

| Defect | Platform | Before (measured) | After (measured) |
|---|---|---|---|
| Popover opened off-screen | Windows | Panel top at `y=1446` on a 1440-tall display — the app's only UI was unreachable | `rect=(2074,916)-(2410,1395)`, sitting above the taskbar |
| Popover closed itself immediately | Windows | `visible=True` → `False` 700 ms later | `visible=True`, still there after 14 s idle |
| Double-rounded corners | Windows | Two curves per corner, acrylic showing through the seam | `--card-radius: 0`; only DWM's curve remains |
| **Settings panel unreachable** | **both** | Card 468 px tall, content 812 px — `Price data / Check for updates / Restore built-in` all sat below the window with **no scrollbar** | Card scrolls; verified by measuring every child element's box against the real window |

**The first two** came from a positioning routine that hard-coded "always below the icon" (right
for a top menu bar, wrong for a bottom taskbar), only clamped X, and used the whole monitor
rather than the work area. The self-closing was a `focused(false)` at construction plus Windows'
foreground lock, so the window never really held focus and the next `Focused(false)` closed a
panel nobody had seen.

**The last two** matter more than they look. `apply_acrylic` takes no radius parameter the way
the two macOS calls do, so on Windows the rounding belongs to DWM and the CSS must not draw it a
second time. And the overflow bug is worth reading in full: the window is a **fixed 320×470,
`resizable(false)`**, and `html, body { overflow: hidden }` together turn "below the window"
into **"unreachable"**. The entire price-sync feature was invisible to users — and **a missing
control does not read as a bug, it reads as a feature the app does not have.**

Details: [`docs/windows.md`](docs/windows.md).

### Two defects that only macOS had

The macOS half was written *after* Windows and looked like the easier target. Both of these are
worth reading because **each one masked the other**:

| Defect | Symptom (measured) | Root cause |
|---|---|---|
| A square outline outside all four corners | The panel is rounded, but the window's rectangular bounds still drew *outside* the curve — hidden along the straight edges, visible only where the corners cut in | `apply_liquid_glass` never passed a `content_view`, so `window-vibrancy`'s `move_primary_content_view` returned on its first line — skipping that crate's **only** `apply_corner_radius_layer` call, which is what sets `masksToBounds` |
| The panel opened in the middle of the screen | `tray.rect()` returns a **zero-height** rect on macOS 26 — `x=0 y=2100 w=82 h=0`, i.e. `{{0,0},{41,0}}` in AppKit terms. That is not a position | Two bugs stacked: the degenerate rect, *and* `monitor_from_point` comparing it against `CGDisplayBounds` (**logical points**) while the rect is in **physical pixels** — so on a 2× display every coordinate doubled and the monitor lookup returned `None` |

The corner fix is `clip_to_card_radius`: set `cornerRadius` + `masksToBounds` on the window's view.
**It only adds clipping; it does not touch the material** — the glass was always meant to be
clipped to that radius.

The positioning fix is honest about its limits. When the icon rect is unusable, `position_under`
anchors to the **right end of the menu bar** instead of leaving the window wherever it happened to
be created. **That is a guess at a sensible position, not a repair** — the panel no longer drifts
into the middle of the screen, but it does not promise to sit under the icon you clicked. Doing
that properly needs the status item's real frame (`ns_status_item()`), which is upstream.

Details: [`docs/macos.md`](docs/macos.md).

---

## Price changes without a new release

Edit the pricing JSON in the config repository, bump `verifiedAt`, commit. **No app release.**

In the settings panel (⚙) there is a **Check for updates** button; pressing it pulls the latest
figures and schedule. Three lines of defence: the structure must deserialise,
`provider` must still be `deepseek`, `verifiedAt` must not go backwards (guards against CDN cache
rollback), and a price move of more than 20× is rejected. **Any single failure rejects the whole
thing** — your current data is kept, and the tray icon is unaffected.

The provenance label in the UI tells you which copy is in use: `Built-in · 2026-09-12` or
`Synced · 2026-09-20`.

### The bundled copy is the floor

Prices and schedule live in configuration, fully decoupled from the engine:

```
crates/schedule/config/providers/deepseek.json   ← compiled in; also the fallback
```

If you never press **Check for updates**, this app behaves **identically** to a version with no
networking at all.

---

## Architecture

```
crates/schedule/          ← pure Rust engine, zero Tauri dependencies, headless-testable
  engine.rs                 window materialisation: state_at / next_transition / day_segments
  model.rs                  config deserialisation types + error types
  validate.rs               can this remote config replace the current one?
  display.rs                duration, clock and relative-date formatting
  view.rs                   compiles config+state into the **exact strings** the UI shows
  config/providers/*.json   prices and schedule
src-tauri/                ← the shell
  tray.rs                   icon, tooltip, context menu
  tick.rs                   background loop: sleeps to the next boundary
  popover.rs                popover creation, positioning, native material
  provider.rs               synced-copy read/write + UPDATE_URL (★ coupled to the CSP)
  autostart.rs              login item: read/write, plus per-launch path healing
  core.rs                   app state + the commands the frontend calls
ui/                       ← popover frontend, static HTML/CSS/JS, no bundler
```

Three deliberate decisions:

**The engine is in Rust, not the frontend.** The icon must keep changing colour while the popover
is closed; an engine in the frontend would need a permanently hidden window. A Rust engine is
also verifiable headlessly with `cargo test`.

**All user-facing text comes from `view.rs`.** The tooltip and the popover are two renderers of
the same facts. Keeping them in one place means they **cannot contradict each other**, and every
turn of phrase, price rounding and countdown string has a unit test. There is not one line of
date arithmetic or number formatting in the frontend.

**Every failure path renders grey, never nothing.** A corrupt config or a bad
`DEEPSEEKBUDGET_FAKE_NOW` shows Unknown plus a reason. The workspace release profile deliberately does
**not** set `panic = "abort"`: it would save a little size but would let any panic in a background
thread kill the process and make the icon vanish. A missing price indicator is far worse than a
grey dot.

---

## Development

Prerequisites: Rust (stable ≥ 1.90), Node (only for the Tauri CLI), and on macOS Xcode Command
Line Tools (**not** full Xcode).

```bash
npm install          # only to get @tauri-apps/cli
npm run dev          # launches; the icon appears in the menu bar

cargo test --workspace                      # 127 tests
cargo test -p deepseekbudget-schedule       # engine only, ~0.6 s

npx tauri build --bundles app               # macOS .app
cargo build --release -p deepseek-budget    # Windows portable exe, no installer
```

### Time travel (the most important development facility)

Peak/off-peak boundaries cannot be verified by waiting — the next one may be hours away, and on a
Friday evening it is **63 hours** away. So "now" can be pinned with an environment variable:

```bash
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T02:00:00Z npm run dev   # Monday 10:00 Beijing → peak (orange)
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T05:00:00Z npm run dev   # Monday 13:00 Beijing → lunch off-peak
DEEPSEEKBUDGET_FAKE_NOW=2026-09-12T02:00:00Z npm run dev   # Saturday → off-peak all day
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T03:59:59Z npm run dev   # one second of peak left → watch it flip
```

It must be RFC 3339. A typo does not crash anything: a warning goes to stderr and the real clock
is used. The variable is only ever set by developers, and a typo should not pin the icon grey
forever.

### Opening the popover directly

The popover is created on the **first tray click**, and that is the moment the native material is
applied. To check the material without a human clicking:

```bash
DEEPSEEKBUDGET_OPEN_POPOVER=1 npm run dev
```

It expands on launch and prints a material report to stderr, e.g.
`DeepSeek Budget: popover material — native glass applied`, or
`none available, falling back to an opaque background`.

## Known limitations

1. **Opening the popover steals focus.** It is an ordinary always-on-top window, so `show()` takes
   focus from whatever you were using.
2. **Timezones are modelled as fixed UTC offsets.** Exact for zones without DST (including the
   UTC+8 audience this is built for); a documented approximation elsewhere. The *judgement* is
   always made in UTC, so **peak/off-peak is always correct** — only the display can be an hour
   off.
3. **The Windows tooltip degrades.** Windows caps tray tooltips at 127 UTF-16 characters and does
   not document multi-line support, so Windows gets a compressed three-line form and the popover
   carries the full information.
4. **Launch-at-login has one unverified hop left.** On Windows, both the written registry value and
   "that command line really does start the tray icon" were checked. On macOS the LaunchAgent is now
   exercised for real — `launchctl bootstrap` loaded it (`state = running`, `runs = 1`), and moving
   the app made the entry heal byte-for-byte. But **neither machine was logged out or restarted**,
   so "the OS runs it at login" is still inference. What remains on macOS is the OS's scheduling
   timing rather than our code, since `launchctl bootstrap` is the same operation launchd performs
   at login.
5. **A moved portable build is only healed on the next launch.** Both platforms store an absolute
   path, so moving the app leaves the entry pointing at nothing. Every launch rewrites it — but the
   login that happens *between* the move and the next launch is simply lost, silently. (On macOS,
   running straight from the download folder, i.e. App Translocation, is the same situation.)
6. **The glass appearance is verified by eye, not by assertion.** `NSGlassEffectView` does not exist
   in headless Chrome, so the code path is checked by the material report and the *appearance* by
   screenshot. Screen-recording permission for `screencapture` was granted on 2026-09-13, which is
   what made the menu bar icon and the popover photographable at all. **Before that, this project
   claimed Windows was the easier platform to verify — that claim is now void.** Platform
   verifiability is a quantity that changes; it is only true as of the date it was measured.
7. **Config-error messages are English only.** They come from `ScheduleError`'s `Display` and are
   not localised. These are developer/config-time errors.
8. **The language is read once at launch.** Changing the system language needs a restart. Switching
   it in the settings panel takes effect immediately, tooltip included.
9. **No single-instance guard.** With launch-at-login on, "it is already running and the user
   double-clicks it again" becomes the normal path rather than a rare one — on Windows that means
   two tray icons. Still unhandled.

---

## Distribution and signing

`npx tauri build --bundles app` produces `target/release/bundle/macos/DeepSeek Budget.app`.

**The `.app` is itself portable** — drag it anywhere and double-click; no installer, no admin
rights. (macOS has no "single-file exe"; a `.app` is its equivalent.)

**But it cannot be sent to someone else as-is.** A build made on your own machine runs; once it
has been downloaded, macOS requires notarisation and the recipient must go through
System Settings → Privacy & Security → Open Anyway. Removing that step needs a paid Apple
Developer account. The current configuration uses **ad-hoc signing** (`"signingIdentity": "-"`),
which is required on Apple Silicon but does not solve the above.

**Build the Windows version on Windows.** Cross-compiling from macOS is not viable: Tauri has no
cross-compilation path, and `rustup target add x86_64-pc-windows-msvc` only gives you `rust-std` —
no `link.exe`, no Windows SDK, no WebView2Loader, and the packaging tools are themselves Windows
executables. Cross-platform release means **two native builds**, not one cross-compile.

---

## The icon

The tray icon is the **DeepSeek whale logo**, one shape in three colours — **the shape says who,
the colour says what price**:

| State | Colour |
|---|---|
| Off-peak | **`#4d6bfe`** — the brand blue taken from the logo file itself |
| Peak | `#ff9500` (Apple systemOrange: a warning that does not glare) |
| Unknown | `#8e8e93` (grey — "no information", not a third price tier) |

Blue and orange are near-complementary, so the two states are distinguishable at a glance and to
the most common forms of colour blindness.

```bash
python3 tools/make-tray-icons.py    # the three tray icons (colours the SVG)
python3 tools/make-app-icon.py      # 1024×1024 app-icon source
npm run icons                       # derives the whole bundle.icon set
```

**Why 49×36 rather than square.** `tray-icon` hard-codes **18 pt high** on macOS and derives the
width from the image's aspect ratio. The whale is wide (viewBox 256×189, ratio 1.354), so in a
square canvas it would be **width**-limited: at 34 px available it can only be
`34 ÷ 1.354 ≈ 25.1 px` (12.6 pt) tall. **Letting the canvas follow the logo's ratio** moves the
bottleneck to height, and the whale fills 34 px (**17.0 pt**) — `34 ÷ 25.1 ≈ 1.35×` larger.

Those numbers (34 px tall, 1 px margin all round) were **measured, not calculated**: decode the
PNG from `tools/make-tray-icons.py` and read the alpha bounding box. Windows uses square tray
slots and scales a wide image by width, so this choice **helps macOS at no cost to Windows**.

**Why rasterise the SVG with Chrome.** It is the only SVG renderer already on the machine (the
offscreen preview uses it too), so the project gains no new dependency — otherwise three 36 px
images would drag in cairosvg or librsvg. The path can be overridden with `$CHROME`.

**One render, straight to final pixels; no supersampling.** The script originally rendered at 8×
and let `sips` shrink it, and **measured worse**. Same 47×34 glyph, same 49×36 canvas:

| Pipeline | Ink (Σα) | Solid px | Blur px | Colours |
|---|---|---|---|---|
| 8× render + `sips` | 191,607 | 592 | 327 | 89 |
| **Direct to final size** | **199,384** | **649** | **262** | **40** |

Ink +4% (less washed out), solid +10%, blur −20%, colours halved (less antialiasing noise).
8×-then-shrink is **two resamplings**, and a small glyph cannot afford it. To reproduce: view both
outputs side by side at 9–10× nearest-neighbour, or measure the four numbers above.

The icon **deliberately does not use template mode**: template renders the image as an alpha-only
monochrome mask, which throws away the blue/orange semantics — and those are the entire point of
the icon. The cost is that it does not auto-invert with a light/dark menu bar; both colours were
verified legible on mid-grey and dark menu bars.

**Comparison against shipping apps (measured on a real `/Applications`).** Sampling is limited to
icons whose filenames are identifiable; anything hidden inside `Assets.car` is not visible:

| App | Resource | Pixels | Form |
|---|---|---|---|
| ChatGPT | `chatgptTemplate@2x.png` | 18 / 36 | monochrome template |
| Cursor | `trayTemplate@2x.png` | 16 / 32 | monochrome template |
| Trae | `tray-icon@2x.png` | 22 / 44 | monochrome template |
| AltTab | `menubar-2@2x.png` | 44 | **colour** (recording state) |
| VideoFusion | `progressbar-icon@2x.png` | 16/24/48 | **colour** (progress state) |

The standard is **16–18 pt tall, @1x+@2x pairs, monochrome template**. This project deviates in
three places, two by choice and one already fixed: 18 pt **matches**; colour is a **deliberate
deviation in service of semantics** (AltTab and VideoFusion set precedent); the missing @1x has
**no practical effect**, since `tray-icon` accepts a single image and every Mac that runs macOS 26
is a Retina machine. **Note that vector PDF is not the menu bar standard** — all 347 small vector
PDFs on this machine are generic UI glyphs such as iWork bullets, and not one menu bar app uses
them.

**The app icon (Finder/Dock) is still the original geometric mark**, not the official logo — that
would raise trademark questions, and it was not requested.

---

## Documentation

Code can be read. **Why it was built this way, and what each platform actually does** cannot.
Three documents cover that layer:

| Document | Contents |
|---|---|
| [`docs/domain-pricing.md`](docs/domain-pricing.md) | The DeepSeek peak/off-peak rules this app encodes (verified against the official page), how timezones are modelled, and **how far the official API actually lets you measure your own spend** |
| [`docs/icon-pipeline.md`](docs/icon-pipeline.md) | How the tray icon is produced, why the canvas is 49×36 rather than square, a measured comparison of two export pipelines, and how the result stacks up against shipping menu bar apps |
| [`docs/windows.md`](docs/windows.md) | What the Windows port actually cost: popover positioning, the foreground lock, why acrylic forces square corners, the 127-character tooltip limit, and three traps in automating Windows UI verification |
| [`docs/macos.md`](docs/macos.md) | The other side of the same story: a `tray.rect()` that returns a zero-height rectangle, a monitor lookup that takes logical points while being handed physical pixels, who owns the corner radius, and a platform-verifiability claim that had to be retracted |

---

## Licence

MIT. See [`LICENSE`](LICENSE).
