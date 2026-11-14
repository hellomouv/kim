"""Generate the Noeul and Ddalgak program icons (ADR 0090).

Developer-only asset tool (not part of any shipped binary). Requires Pillow.

    python -I assets/brand/make_tool_icons.py

Both icons share Subak's tile (assets/subak/subak.svg): a 40-unit rounded square in bg.overlay
(#1C1B24, rx 9.5, a 1-unit #26252F edge) with one flat, high-contrast glyph:

* Noeul (노을, the evening glow): a striped setting sun on a horizon with its reflection.
* Ddalgak (딸깍, the sound of a click): a mint keycap pressed into its base, with click marks.

The SVG masters below are the source of truth for the geometry; this script draws the same
shapes supersampled and writes the PNG sizes and a PNG-compressed multi-size .ico.
"""

import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
SS = 16  # supersampling factor over the 256 px master
PNG_SIZES = [16, 20, 24, 32, 48, 64, 128, 256]
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]

TILE = "#1C1B24"
EDGE = "#26252F"


def rgba(h, a=255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


def canvas():
    n = 256 * SS
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img), n / 40.0


def tile(d, u):
    d.rounded_rectangle([0.5 * u, 0.5 * u, 39.5 * u, 39.5 * u], radius=9.5 * u, fill=rgba(EDGE))
    d.rounded_rectangle([1.5 * u, 1.5 * u, 38.5 * u, 38.5 * u], radius=8.5 * u, fill=rgba(TILE))


def noeul():
    img, d, u = canvas()
    tile(d, u)
    cx, horizon, r = 20.0, 23.0, 11.0
    # The sun: a half disc above the horizon in three warm bands, with thin gaps (stripes).
    sun = Image.new("L", img.size, 0)
    ImageDraw.Draw(sun).pieslice([(cx - r) * u, (horizon - r) * u, (cx + r) * u, (horizon + r) * u], 180, 360, fill=255)
    bands = [(horizon - r, horizon - 6.2, "#FFC86B"), (horizon - 5.4, horizon - 2.6, "#FF8A5B"), (horizon - 1.9, horizon, "#F2545B")]
    color = Image.new("RGBA", img.size, (0, 0, 0, 0))
    cd = ImageDraw.Draw(color)
    for top, bottom, col in bands:
        cd.rectangle([0, top * u, img.size[0], bottom * u], fill=rgba(col))
    img.paste(color, (0, 0), Image.composite(color, Image.new("RGBA", img.size, (0, 0, 0, 0)), sun).split()[3])
    # The horizon.
    d.rounded_rectangle([6.5 * u, (horizon + 0.6) * u, 33.5 * u, (horizon + 1.6) * u], radius=0.5 * u, fill=rgba("#6E6B7C"))
    # The reflection: three bars, narrowing and fading.
    for i, (w, col, a) in enumerate([(16.0, "#FF8A5B", 230), (10.0, "#F2545B", 200), (5.0, "#F2545B", 150)]):
        y = horizon + 4.2 + i * 3.0
        d.rounded_rectangle([(cx - w / 2) * u, y * u, (cx + w / 2) * u, (y + 1.5) * u], radius=0.75 * u, fill=rgba(col, a))
    return img


def ddalgak():
    img, d, u = canvas()
    tile(d, u)
    # The keycap: a base (its sides) and the pressed top face, offset down a little.
    d.rounded_rectangle([8.5 * u, 13.0 * u, 29.5 * u, 33.0 * u], radius=4.5 * u, fill=rgba("#3A3847"))
    d.rounded_rectangle([10.0 * u, 13.5 * u, 28.0 * u, 29.5 * u], radius=3.8 * u, fill=rgba("#7FD1B9"))
    d.rounded_rectangle([12.0 * u, 15.0 * u, 26.0 * u, 18.0 * u], radius=1.5 * u, fill=rgba("#A8E6D3"))
    # The click: three short marks fanning out from the top-right corner.
    w = 1.6 * u
    for (x0, y0, x1, y1) in [(31.0, 11.0, 34.0, 8.0), (32.2, 15.0, 35.6, 14.2), (27.2, 9.6, 27.8, 6.0)]:
        d.line([x0 * u, y0 * u, x1 * u, y1 * u], fill=rgba("#F1E7D0"), width=int(w))
        for x, y in [(x0, y0), (x1, y1)]:
            d.ellipse([x * u - w / 2, y * u - w / 2, x * u + w / 2, y * u + w / 2], fill=rgba("#F1E7D0"))
    return img


def write(name, master):
    out = ROOT / "assets" / name
    out.mkdir(parents=True, exist_ok=True)
    pngs = {}
    for s in sorted(set(PNG_SIZES + ICO_SIZES)):
        im = master.resize((s, s), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, "PNG", optimize=True)
        pngs[s] = buf.getvalue()
        if s in PNG_SIZES:
            (out / f"{name}-{s}.png").write_bytes(pngs[s])
    entries = [pngs[s] for s in ICO_SIZES]
    head = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    dirs = b""
    for s, data in zip(ICO_SIZES, entries):
        dirs += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    (out / f"{name}.ico").write_bytes(head + dirs + b"".join(entries))
    print(f"wrote assets/{name}: {len(PNG_SIZES)} PNGs and {name}.ico")


if __name__ == "__main__":
    write("noeul", noeul())
    write("ddalgak", ddalgak())
