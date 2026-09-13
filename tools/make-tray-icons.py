#!/usr/bin/env python3
"""Generate the three tray icons from the official DeepSeek logo.

Output is 49x36 RGBA PNGs, one per pricing state, recoloured from a single source:
`assets/deepseek-logo.svg`. The logo is the shape; the colour is the state.

**Why 36 tall.** `tray-icon` (which Tauri wraps) hardcodes an 18pt height on macOS and scales
the source to fit. 18pt at 2x is 36px, so 36px is the pixel-exact source height and the mark
stays crisp instead of being resampled from some other size.

**Why 49 wide, and not square.** The width follows the logo's own proportions. See the note
on canvas shape below — a square canvas would waste most of the available height.

**Why Chrome rasterises the SVG.** It is the only SVG renderer already on this machine, and
the repo already depends on it for the offscreen preview harness — so this adds no new
dependency to the project. The alternative (cairosvg, librsvg) would mean a new toolchain
just to draw three 36px images. Chrome can be overridden with $CHROME.

**Render once, at the final size. Do not supersample and downscale.** This script used to
render at 8x and hand the result to `sips` to shrink. That was measurably worse: comparing
the shipped PNG against the alternatives at 9x nearest-neighbour, the 8x+sips path lost ~4%
of its ink (sum of alpha) and produced ~30% more partially-transparent edge pixels, i.e. it
washed the mark out and smeared its edges. Rendering once at the target size — letting the
rasteriser antialias straight onto the final pixel grid — kept the most ink and the fewest
blurred pixels of every pipeline tried, including gamma-correct box downsampling of the same
8x render. The lesson: for a small glyph, one resample is better than two.

**Why colour is preserved.** These are NOT macOS template images: `popover`/`tray` pass
`is_template = false`, because a template render uses only the alpha channel and would throw
the blue/orange distinction away — which is the entire point of the icon.

Run:  python3 tools/make-tray-icons.py
"""

import os
import re
import subprocess
import sys
import tempfile

# The logo's own proportions (viewBox="0 0 256 189"), which the canvas follows rather than
# fighting. See the note on canvas shape below.
LOGO_W, LOGO_H = 256, 189

# 18pt * 2 (retina) — the exact height tray-icon renders at on macOS.
HEIGHT = 36
MARGIN = 1  # px of clear space at 1x

# **The canvas is wide, not square, and that is deliberate.**
#
# `tray-icon` on macOS hardcodes an 18pt *height* and derives the width from the image's
# aspect ratio. In a square canvas the whale would be constrained by *width* instead, and end
# up only 34/1.354 ≈ 25px (12.6pt) tall. Matching the canvas to the logo's proportions swaps
# the binding constraint to height, so the mark fills 34px (17.0pt) — 1.35x larger, and
# noticeably easier to read at a glance, which is the entire job of this icon.
#
# On Windows the tray slot is square, so a wide image is scaled to fit its width and ends up
# the same size either way. This choice helps macOS and costs Windows nothing.
CANVAS_W = round(HEIGHT * LOGO_W / LOGO_H)
CANVAS_H = HEIGHT

CHROME = os.environ.get(
    "CHROME", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
)

# State -> colour.
#
# Off-peak takes DeepSeek's own brand blue, straight out of the logo file, so the "cheap"
# state is also the on-brand one. Peak keeps Apple's systemOrange, which reads as a warning
# without being alarming; blue and orange are near-complementary, so the two states are
# distinguishable at a glance even for the most common form of colour blindness.
#
# `unknown` has no colour of its own: it means the app could not work out the state, so it
# deliberately reads as "no information" rather than as a third tier.
STATES = {
    "offpeak": "#4d6bfe",
    "peak": "#ff9500",
    "unknown": "#8e8e93",
}

# The whale is drawn at the canvas's full height and its own aspect ratio — never stretched to
# fill a differently-shaped box, which would visibly deform the mark. Because the canvas already
# matches the logo, no letterboxing is needed and the height is used in full.


def render(svg_markup, colour, out_path):
    """Rasterise the SVG directly at the final pixel size and write the PNG."""
    recoloured = re.sub(r'fill="[^"]*"', f'fill="{colour}"', svg_markup, count=1)

    glyph_w = CANVAS_W - 2 * MARGIN
    glyph_h = CANVAS_H - 2 * MARGIN

    page = f"""<!doctype html>
<html><head><meta charset="utf-8"><style>
  html, body {{ margin: 0; padding: 0; background: transparent; }}
  /* The SVG must be *centred* in the canvas, not dropped at its top-left. Left to itself a
     block element sits flush against both edges, which silently moved the whole MARGIN onto
     the right and bottom — the mark ended up touching the top-left corner while claiming to
     have 1px of clear space all round. Flex centring is what actually buys the symmetric
     margin this script's MARGIN constant promises. */
  body {{
    width: {CANVAS_W}px; height: {CANVAS_H}px;
    display: flex; align-items: center; justify-content: center;
  }}
  /* Sized to the canvas minus the margin, at the logo's own proportions, so the mark fills
     the height and preserveAspectRatio never has to letterbox it. These are *final* pixels:
     the rasteriser antialiases the vector straight onto the grid the menu bar will use, so
     nothing resamples it afterwards. */
  svg {{ display: block; width: {glyph_w}px; height: {glyph_h}px; }}
</style></head>
<body>{recoloured}</body></html>
"""

    with tempfile.TemporaryDirectory() as work:
        page_path = os.path.join(work, "icon.html")
        with open(page_path, "w", encoding="utf-8") as handle:
            handle.write(page)

        subprocess.run(
            [
                CHROME,
                "--headless=new",
                "--disable-gpu",
                "--hide-scrollbars",
                "--force-device-scale-factor=1",
                # 8 hex digits: fully transparent background, which is what lets the menu bar
                # show through around the mark.
                "--default-background-color=00000000",
                f"--window-size={CANVAS_W},{CANVAS_H}",
                f"--screenshot={out_path}",
                f"file://{page_path}",
            ],
            check=True,
            capture_output=True,
        )


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(here)
    logo_path = os.path.join(root, "assets", "deepseek-logo.svg")
    out_dir = os.path.join(root, "src-tauri", "icons")

    if not os.path.exists(CHROME):
        sys.exit(
            f"Chrome not found at {CHROME!r}.\n"
            "It is needed to rasterise the SVG. Set $CHROME to your browser binary."
        )
    with open(logo_path, encoding="utf-8") as handle:
        svg_markup = handle.read()

    os.makedirs(out_dir, exist_ok=True)
    for state, colour in STATES.items():
        out_path = os.path.join(out_dir, f"tray-{state}.png")
        render(svg_markup, colour, out_path)
        print(
            f"wrote src-tauri/icons/tray-{state}.png  "
            f"({CANVAS_W}x{CANVAS_H}, {colour}, {os.path.getsize(out_path)} bytes)"
        )


if __name__ == "__main__":
    main()
