#!/usr/bin/env python3
"""Render the real popover UI offscreen, without launching the app.

Why this exists: the popover is only reachable by clicking a menu bar icon, and capturing the
screen needs Screen Recording permission. That leaves a real bug class unverified — a
mismatch between the camelCase field names Rust emits and the ones `ui/main.js` reads would
produce a blank or half-empty panel that no unit test would catch.

So this composes a standalone page out of the *actual* `ui/index.html`, `ui/styles.css` and
`ui/main.js` — read at generation time, never copied, so they cannot drift — stubs the two
`window.__TAURI__` calls the UI makes, and hands the result to headless Chrome.

Usage:
    python3 tools/make-preview.py <payload.json> <out.html> [--dark]

Pair it with the Rust example that emits the payload:

    cargo run -q -p deepseekbudget-schedule --example view -- --json 2026-09-14T02:00:00Z 480 CNY \\
        > /tmp/peak.json
    python3 tools/make-preview.py /tmp/peak.json /tmp/peak.html

    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \\
        --screenshot=/tmp/peak.png --window-size=320,470 --force-device-scale-factor=2 \\
        --hide-scrollbars file:///tmp/peak.html
"""

import json
import os
import re
import sys

DARK_MARKER = "@media (prefers-color-scheme: dark)"


def read(relative_path):
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    with open(os.path.join(root, relative_path), encoding="utf-8") as handle:
        return handle.read()


def host_platform():
    """The `platform` the frontend would be told on this machine.

    The start-at-login row names a different mechanism on each platform, and the two sentences
    are different lengths — a hardcoded value previews text this machine never renders.
    """
    if sys.platform == "darwin":
        return "macos"
    if sys.platform.startswith("win"):
        return "windows"
    return "linux"


def force_dark(css):
    """Rewrite the `prefers-color-scheme: dark` block so it applies unconditionally.

    Chrome's headless mode has no clean flag for emulating the media feature, and
    duplicating the dark palette here would let it drift from styles.css. So the block is
    unwrapped in place, using brace matching rather than a regex so a nested block cannot
    be cut short.
    """
    start = css.find(DARK_MARKER)
    if start == -1:
        sys.exit(
            f"styles.css no longer contains {DARK_MARKER!r}; "
            "update force_dark() in this script"
        )
    open_brace = css.index("{", start)
    depth = 0
    for index in range(open_brace, len(css)):
        if css[index] == "{":
            depth += 1
        elif css[index] == "}":
            depth -= 1
            if depth == 0:
                inner = css[open_brace + 1 : index]
                return css[:start] + "/* forced dark for preview */" + inner + css[index + 1 :]
    sys.exit("unbalanced braces in styles.css")


def extract_body(html):
    match = re.search(r"<body>(.*)</body>", html, re.S)
    if not match:
        sys.exit("ui/index.html has no <body>")
    # The real page loads main.js as an external file; the preview inlines it instead.
    return re.sub(r'\s*<script src="main\.js"></script>', "", match.group(1))


def stub_settings(view):
    """Mirror what `core::settings_view` returns, derived from the payload."""
    return {
        "currency": view.get("currency", "CNY"),
        "availableCurrencies": view.get("availableCurrencies", []),
        "timezoneOverrideMinutes": None,
        "systemOffsetMinutes": 480,
        "zoneLabel": view.get("displayZoneLabel", "System"),
        "offsetLabel": view.get("offsetLabel", ""),
        "timezoneChoices": [
            {"minutes": None, "label": f"System default · {view.get('offsetLabel', '')}"},
            {"minutes": 0, "label": "UTC"},
            {"minutes": 480, "label": "UTC+08:00"},
            {"minutes": -300, "label": "UTC-05:00"},
        ],
        "language": view.get("locale"),
        "resolvedLanguage": view.get("locale", "en"),
        "languageChoices": [
            {"tag": None, "label": "System default · English"},
            {"tag": "zh", "label": "中文"},
            {"tag": "en", "label": "English"},
        ],
    }


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 2:
        sys.exit(__doc__)
    payload_path, out_path = args
    dark = "--dark" in sys.argv

    with open(payload_path, encoding="utf-8") as handle:
        view = json.load(handle)

    css = force_dark(read("ui/styles.css")) if dark else read("ui/styles.css")

    # `data-glass="off"` is a statement of fact, not a workaround: headless Chrome has no
    # NSGlassEffectView behind it, so the card must use its opaque background — the same
    # fallback the real app uses when no material applies. This also keeps the preview
    # readable, which is the point of generating it.
    html = f"""<!doctype html>
<html lang="en" data-glass="off">
<head>
<meta charset="utf-8" />
<title>DeepSeek Budget preview</title>
<style>
{css}
/* Preview only: pin the page to the real popover size so the screenshot matches. */
html, body {{ width: 320px; height: 470px; }}
</style>
</head>
<body>
{extract_body(read("ui/index.html"))}
<script>
// Minimal stand-in for the Tauri bridge: the only calls the UI makes.
const VIEW = {json.dumps(view)};
const SETTINGS = {json.dumps(stub_settings(view))};
const ENVIRONMENT = {{ glass: false, platform: "{host_platform()}" }};
const AUTOSTART = {{ enabled: false, blocked: false, supported: true, error: null }};
window.__TAURI__ = {{
  core: {{
    invoke: async (cmd) => {{
      if (cmd === "get_view") return VIEW;
      if (cmd === "get_settings") return SETTINGS;
      if (cmd === "get_environment") return ENVIRONMENT;
      if (cmd === "get_autostart") return AUTOSTART;
      return null;
    }},
  }},
  event: {{ listen: async () => () => {{}} }},
}};
</script>
<script>
{read("ui/i18n.js")}
</script>
<script>
{read("ui/main.js")}
</script>
</body>
</html>
"""

    with open(out_path, "w", encoding="utf-8") as handle:
        handle.write(html)
    theme = "dark" if dark else "light"
    print(
        f"wrote {out_path} ({theme}, {view.get('locale')}) — "
        f"state={view.get('stateLabel') or 'Unknown'}"
    )


if __name__ == "__main__":
    main()
