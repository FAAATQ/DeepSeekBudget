#!/usr/bin/env python3
"""Generate the 1024x1024 source image for the application icon.

This is the *app* icon (Finder, Dock, the .app bundle) — a different thing from the tray
icons, which `make-tray-icons.py` handles. It is a separate script because the two have
opposite constraints: the tray icon must be a bare disc that disappears into the menu bar,
while the app icon should look deliberate at large sizes.

Feed the output to the Tauri CLI, which derives the whole `bundle.icon` set
(`32x32.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, ...):

    python3 tools/make-app-icon.py
    npx tauri icon src-tauri/app-icon.png

Run:  python3 tools/make-app-icon.py
"""

import os
import struct
import zlib

SIZE = 1024
SUPERSAMPLE = 2  # 2x2 per pixel is plenty at this resolution

# Matches the brand colour already used in this project's editor theme.
BACKGROUND = (0x00, 0x08, 0xFF)
FOREGROUND = (0xFF, 0xFF, 0xFF)

# macOS app icons are not full-bleed: leave a margin and use a generous corner radius so the
# shape reads as the platform's rounded square rather than a hard rectangle.
INSET = 0.094
CORNER_RATIO = 0.2237  # Apple's squircle approximation
DISC_RATIO = 0.30


def write_png(path, pixels, size):
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = bytearray()
    for row in pixels:
        raw.append(0)
        for r, g, b, a in row:
            raw += bytes((r, g, b, a))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as handle:
        handle.write(png)


def rounded_box_distance(px, py, half, radius):
    """Signed distance from (px, py) to a rounded box centred on the origin."""
    dx = abs(px) - (half - radius)
    dy = abs(py) - (half - radius)
    outside = (max(dx, 0.0) ** 2 + max(dy, 0.0) ** 2) ** 0.5
    return outside + min(max(dx, dy), 0.0) - radius


def main():
    centre = SIZE / 2.0
    half = SIZE / 2.0 - SIZE * INSET
    radius = SIZE * CORNER_RATIO
    disc_radius = SIZE * DISC_RATIO

    rows = []
    for y in range(SIZE):
        row = []
        for x in range(SIZE):
            box_hits = 0
            disc_hits = 0
            for sy in range(SUPERSAMPLE):
                for sx in range(SUPERSAMPLE):
                    px = x + (sx + 0.5) / SUPERSAMPLE - centre
                    py = y + (sy + 0.5) / SUPERSAMPLE - centre
                    if rounded_box_distance(px, py, half, radius) <= 0:
                        box_hits += 1
                        if (px * px + py * py) ** 0.5 <= disc_radius:
                            disc_hits += 1
            total = SUPERSAMPLE * SUPERSAMPLE
            if box_hits == 0:
                row.append((0, 0, 0, 0))
                continue
            alpha = box_hits / total
            disc = disc_hits / box_hits  # fraction of the covered box that is disc
            r = round(BACKGROUND[0] * (1 - disc) + FOREGROUND[0] * disc)
            g = round(BACKGROUND[1] * (1 - disc) + FOREGROUND[1] * disc)
            b = round(BACKGROUND[2] * (1 - disc) + FOREGROUND[2] * disc)
            row.append((r, g, b, round(alpha * 255)))
        rows.append(row)

    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(here, "..", "src-tauri", "app-icon.png")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    write_png(path, rows, SIZE)
    print(f"wrote {os.path.relpath(path, os.path.join(here, '..'))} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()
