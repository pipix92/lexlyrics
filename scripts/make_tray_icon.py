#!/usr/bin/env python3
"""Genera l'icona tray di LexLyrics: una doppia nota musicale (♫).

Scrive, a partire dalla stessa geometria:
  res/icons/hicolor/scalable/apps/lexlyrics-tray.svg   (tema icone, nitida a ogni scala)
  res/icons/hicolor/<N>x<N>/apps/lexlyrics-tray.png    (fallback raster)
  res/tray/lexlyrics-tray-<N>.argb                     (pixmap ARGB32 incorporata nel binario)

Uso:  python3 scripts/make_tray_icon.py
"""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
NAME = "lexlyrics-tray"
VIEW = 64  # lato del viewBox

CYAN = (56, 189, 248)     # #38bdf8, lo stesso della riga tradotta nel tema karaoke
VIOLET = (168, 85, 247)   # #a855f7
OUTLINE = (11, 16, 32)    # contorno scuro: leggibile anche su pannelli chiari

# geometria nel viewBox 64x64
HEADS = [(18.0, 49.0), (46.0, 43.0)]   # centri delle teste
HEAD_RX, HEAD_RY, HEAD_TILT = 9.5, 6.6, -22.0
STEMS = [(24.2, 13.0, 4.6, 36.0), (52.2, 7.0, 4.6, 36.0)]  # x, y, w, h
BEAM = [(24.2, 11.0), (56.8, 3.0), (56.8, 12.0), (24.2, 20.0)]

PNG_SIZES = [16, 22, 24, 32, 48, 64, 128, 256]
ARGB_SIZES = [32, 48, 64]


def svg() -> str:
    heads = "\n".join(
        f'    <ellipse cx="{x}" cy="{y}" rx="{HEAD_RX}" ry="{HEAD_RY}" '
        f'transform="rotate({HEAD_TILT} {x} {y})"/>'
        for x, y in HEADS
    )
    stems = "\n".join(f'    <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="1"/>' for x, y, w, h in STEMS)
    beam = " ".join(f"{x},{y}" for x, y in BEAM)
    shape = f"{heads}\n{stems}\n    <polygon points=\"{beam}\"/>"
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="0" y1="1" x2="1" y2="0">
      <stop offset="0" stop-color="#{CYAN[0]:02x}{CYAN[1]:02x}{CYAN[2]:02x}"/>
      <stop offset="1" stop-color="#{VIOLET[0]:02x}{VIOLET[1]:02x}{VIOLET[2]:02x}"/>
    </linearGradient>
  </defs>
  <!-- LexLyrics: doppia nota musicale -->
  <g fill="#{OUTLINE[0]:02x}{OUTLINE[1]:02x}{OUTLINE[2]:02x}" stroke="#{OUTLINE[0]:02x}{OUTLINE[1]:02x}{OUTLINE[2]:02x}" stroke-width="3" stroke-linejoin="round">
{shape}
  </g>
  <g fill="url(#g)">
{shape}
  </g>
</svg>
"""


def ellipse_points(cx: float, cy: float, scale: float, steps: int = 96) -> list[tuple[float, float]]:
    t = math.radians(HEAD_TILT)
    pts = []
    for i in range(steps):
        a = 2 * math.pi * i / steps
        x, y = HEAD_RX * math.cos(a), HEAD_RY * math.sin(a)
        pts.append(((cx + x * math.cos(t) - y * math.sin(t)) * scale,
                    (cy + x * math.sin(t) + y * math.cos(t)) * scale))
    return pts


def mask(side: int) -> Image.Image:
    s = side / VIEW
    m = Image.new("L", (side, side), 0)
    d = ImageDraw.Draw(m)
    for cx, cy in HEADS:
        d.polygon(ellipse_points(cx, cy, s), fill=255)
    for x, y, w, h in STEMS:
        d.rounded_rectangle([x * s, y * s, (x + w) * s, (y + h) * s], radius=s, fill=255)
    d.polygon([(x * s, y * s) for x, y in BEAM], fill=255)
    return m


def gradient(side: int) -> Image.Image:
    g = Image.new("RGB", (side, side))
    px = g.load()
    for y in range(side):
        for x in range(side):
            k = (x + (side - 1 - y)) / (2 * (side - 1))  # basso-sinistra -> alto-destra
            px[x, y] = tuple(round(a + (b - a) * k) for a, b in zip(CYAN, VIOLET))
    return g


def render(size: int) -> Image.Image:
    ss = 8  # supersampling
    side = size * ss
    shape = mask(side)
    border = shape.filter(ImageFilter.MaxFilter(2 * round(1.5 * side / VIEW) + 1))

    out = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    out.paste(Image.new("RGBA", (side, side), OUTLINE + (255,)), mask=border)
    out.paste(gradient(side).convert("RGBA"), mask=shape)
    return out.resize((size, size), Image.LANCZOS)


def to_argb(img: Image.Image) -> bytes:
    """ARGB32 in network byte order, il formato di IconPixmap (StatusNotifierItem)."""
    r, g, b, a = img.split()
    return Image.merge("RGBA", (a, r, g, b)).tobytes()


def main() -> None:
    scalable = ROOT / "res/icons/hicolor/scalable/apps" / f"{NAME}.svg"
    scalable.parent.mkdir(parents=True, exist_ok=True)
    scalable.write_text(svg())

    for size in PNG_SIZES:
        out = ROOT / f"res/icons/hicolor/{size}x{size}/apps" / f"{NAME}.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        render(size).save(out)

    tray = ROOT / "res/tray"
    tray.mkdir(parents=True, exist_ok=True)
    for size in ARGB_SIZES:
        (tray / f"{NAME}-{size}.argb").write_bytes(to_argb(render(size)))

    print(f"icone {NAME} generate in res/")


if __name__ == "__main__":
    main()
