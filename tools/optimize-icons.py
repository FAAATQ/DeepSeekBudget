#!/usr/bin/env python3
"""Shrink the app icons — `icon.icns` and `icon.ico` — without touching how they look.

Why this exists
---------------

`tauri icon` writes **true-colour** PNGs (RGBA, `samplesPerPixel: 4`) into the `.icns`, and it
does not try very hard to compress them. For this project's artwork — flat fills, hard outlines,
one circle — that is the wrong encoder by a wide margin:

    1024×1024 slice   607,908 bytes   as written by `tauri icon`
    1024×1024 slice   222,114 bytes   after palette + full compression

The whole `.icns` goes from **1.29 MB to 557 KB**, and the download carries that difference: the
universal macOS zip drops by about 1 MB. That matters for a project whose entire pitch is "a few
megabytes, no installer".

The Windows icon has exactly the same problem and the same fix — its six true-colour PNGs go from
**84 KB to 41 KB**. Windows embeds the `.ico` in the `.exe`, so that weight lands in the download
there too.

What it does
------------

`iconutil` extracts the `.icns` into an `.iconset` of ten PNGs. Each one is re-encoded to a
256-colour palette with full compression, and `iconutil` packs them back. Palette is safe here
because the artwork has no photographic content — measured against the source, the mean channel
difference is **1.0/255** and the worst pixel (**48/255**) sits on a nearly-transparent edge where
the colour is not visible anyway.

The palette step is deliberate rather than plain re-compression: re-saving the same true-colour
pixels only buys about 9%, because the bytes are already there. It is the colour depth that is
wasteful, not the DEFLATE settings.

Run it after `tauri icon`, never instead of it — this script does not generate sizes, it only
recompresses the ones `tauri icon` produced. `npm run icons` chains the two.

**The two halves have different platform requirements, and the script says so rather than
crashing.** `.icns` is repacked by `iconutil`, which exists only on macOS. `.ico` is rewritten in
pure Python and runs anywhere — and it is the *Windows* one, so a Windows build was the one case
where the script fell over before reaching the half that would have helped it. When `iconutil` is
missing the `.icns` half is skipped with a note and the `.ico` half still runs.
"""

import glob
import io
import os
import shutil
import struct
import subprocess
import sys
import tempfile

from PIL import Image

ICNS = os.path.join("src-tauri", "icons", "icon.icns")
ICO = os.path.join("src-tauri", "icons", "icon.ico")
COLORS = 256


def have(command: str) -> bool:
    """Whether `command` is on PATH. `shutil.which`, so Windows finds `iconutil.cmd` too."""
    return shutil.which(command) is not None


def build_iconset(icns: str, iconset: str) -> None:
    subprocess.run(["iconutil", "-c", "iconset", icns, "-o", iconset], check=True)


def recompress(iconset: str) -> tuple[int, int]:
    before = after = 0
    for path in sorted(glob.glob(os.path.join(iconset, "*.png"))):
        was = os.path.getsize(path)
        image = Image.open(path).convert("RGBA")
        # FASTOCTREE is the only quantiser Pillow allows for RGBA. No dithering: the fills are
        # flat, and dithering would trade a real gain in bytes for visible grain in them.
        quantised = image.quantize(colors=COLORS, method=Image.FASTOCTREE, dither=Image.NONE)
        quantised.convert("RGBA").save(path, optimize=True, compress_level=9)
        now = os.path.getsize(path)
        before += was
        after += now
        print(f"  {os.path.basename(path):<26} {was:>9,} -> {now:>9,}")
    return before, after


def recompress_ico(path: str) -> tuple[int, int]:
    """The same waste lives in the Windows icon: six true-colour PNGs, the 256×256 alone 64 KB.

    Rewrites the PNG payloads and leaves the *shape* of the directory entries alone — same sizes,
    still PNG rather than BMP — so the result stays the kind of `.ico` that `tauri icon` produced
    and Windows keeps reading it identically. Only the byte counts move.
    """
    before = os.path.getsize(path)
    with open(path, "rb") as fh:
        data = fh.read()

    reserved, kind, count = struct.unpack("<HHH", data[:6])
    if reserved != 0 or kind != 1:
        raise ValueError(f"{path} is not an .ico")

    entries: list[list[int]] = []
    payloads: list[bytes] = []
    for i in range(count):
        off = 6 + i * 16
        width, height, colors, pad, planes, bits, size, offset = struct.unpack(
            "<BBBBHHII", data[off : off + 16]
        )
        blob = data[offset : offset + size]
        if blob[:8] == b"\x89PNG\r\n\x1a\n":
            image = Image.open(io.BytesIO(blob)).convert("RGBA")
            quantised = image.quantize(colors=COLORS, method=Image.FASTOCTREE, dither=Image.NONE)
            out = io.BytesIO()
            quantised.convert("RGBA").save(out, "PNG", optimize=True, compress_level=9)
            blob = out.getvalue()
        entries.append([width, height, colors, pad, planes, bits, len(blob), 0])
        payloads.append(blob)

    header = struct.pack("<HHH", 0, 1, count)
    offset = len(header) + count * 16
    directory = b""
    for entry in entries:
        entry[7] = offset
        directory += struct.pack("<BBBBHHII", *entry)
        offset += entry[6]

    with open(path, "wb") as fh:
        fh.write(header + directory + b"".join(payloads))

    after = os.path.getsize(path)
    return before, after


def main() -> int:
    if not os.path.exists(ICNS):
        print(f"{ICNS} not found — run `npm run icons` first", file=sys.stderr)
        return 1

    if have("iconutil"):
        original = os.path.getsize(ICNS)
        with tempfile.TemporaryDirectory() as tmp:
            iconset = os.path.join(tmp, "icon.iconset")
            build_iconset(ICNS, iconset)
            before, after = recompress(iconset)
            # Packed by `iconutil`, not by hand. A hand-written container was tried first and is
            # *not* read back correctly — `iconutil -c iconset` recovered 1 of the 10 slices from
            # it, so the padding convention is easy to get subtly wrong. What matters for
            # reproducibility is the pipeline, not this call: `npm run icons` always regenerates
            # from `tauri icon` first, so the input to this script is always the pristine set.
            # Running it twice in a row *without* regenerating drifts, because iconutil
            # re-encodes on the way out.
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", ICNS], check=True)

        final = os.path.getsize(ICNS)
        print()
        print(f"  slices {before:,} -> {after:,} bytes")
        print(f"  {ICNS} {original:,} -> {final:,} bytes  ({100 * (1 - final / original):.1f}% smaller)")
    else:
        # Not a failure. `iconutil` ships with macOS; the `.ico` half below is pure Python and
        # is the one this platform actually ships.
        print(
            "  iconutil not found — skipping the .icns half (it is part of macOS).",
            file=sys.stderr,
        )

    if os.path.exists(ICO):
        was, now = recompress_ico(ICO)
        print(f"  {ICO} {was:,} -> {now:,} bytes  ({100 * (1 - now / was):.1f}% smaller)")
    elif not have("iconutil"):
        print(f"{ICO} not found either — nothing to do", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
