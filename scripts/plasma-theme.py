#!/usr/bin/env python3
"""Writes the Plasma style's frame SVGs (os/files/usr/share/plasma/desktoptheme/tripppin-tiles).

The "Tiles" look: flat, square (radius 0), solid colour, no shadow and no glow. Plasma draws
panels, dialogs, tooltips and the task buttons from nine-patch SVGs (corners, edges, centre and
`hint-*-margin` rectangles); ours are plain rectangles in the design tokens, all vector.
Everything else in the theme falls back to Breeze's. Run from the repository root:
python3 scripts/plasma-theme.py
"""
import pathlib

OUT = pathlib.Path("os/files/usr/share/plasma/desktoptheme/tripppin-tiles")

INK = "#0a0d10"  # taskbar
BG = "#101418"
RAISED = "#1a2025"
LINE = "#2a3238"
ACCENT = "#3fae7a"
GREEN = "#1f6b45"


def frame(prefix, fill, margin, border=None, underline=None, fill_opacity=1.0):
    """A nine-patch in 6 x 6 units: 2-unit corners and edges, a 2 x 2 centre.

    `border` is a 1 px outline colour; `underline` a 2 px stripe along the bottom edge.
    Corners and edges are plain rectangles, so the frame is square at any scale."""
    p = prefix
    cells = {
        "topleft": (0, 0), "top": (2, 0), "topright": (4, 0),
        "left": (0, 2), "center": (2, 2), "right": (4, 2),
        "bottomleft": (0, 4), "bottom": (2, 4), "bottomright": (4, 4),
    }
    parts = []
    for name, (x, y) in cells.items():
        el = f'<rect x="{x}" y="{y}" width="2" height="2" fill="{fill}" fill-opacity="{fill_opacity}"/>'
        extra = ""
        if border:
            if y == 0:
                extra += f'<rect x="{x}" y="0" width="2" height="1" fill="{border}"/>'
            if y == 4:
                extra += f'<rect x="{x}" y="5" width="2" height="1" fill="{border}"/>'
            if x == 0:
                extra += f'<rect x="0" y="{y}" width="1" height="2" fill="{border}"/>'
            if x == 4:
                extra += f'<rect x="5" y="{y}" width="1" height="2" fill="{border}"/>'
        if underline and y == 4:
            extra += f'<rect x="{x}" y="4" width="2" height="2" fill="{underline}"/>'
        # The id'd group holds the fill and any outline: Plasma renders an element by id.
        parts.append(f'<g id="{p}{name}">{el}{extra}</g>')
    for side in ("top", "bottom", "left", "right"):
        parts.append(f'<rect id="{p}hint-{side}-margin" x="0" y="0" width="{margin if side in ("left", "right") else 1}" height="{margin if side in ("top", "bottom") else 1}" fill="none"/>')
    parts.append(f'<rect id="{p}hint-stretch-borders" x="0" y="0" width="1" height="1" fill="none"/>')
    return "\n  ".join(parts)


def svg(body):
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="6" height="6" viewBox="0 0 6 6">
  {body}
</svg>
"""


def write(rel, text):
    path = OUT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


# Taskbar: solid #0a0d10, no margin, so the buttons sit flush top to bottom.
write("widgets/panel-background.svg", svg(frame("", INK, 0) + "\n  " + frame("floating-", INK, 0)))
# Pop-ups (tray, calendar, volume, OSD): the raised surface, a hairline.
write("dialogs/background.svg", svg(frame("", RAISED, 8, border=LINE)))
write("widgets/background.svg", svg(frame("", RAISED, 6)))
write("widgets/tooltip.svg", svg(frame("", RAISED, 6, border=LINE)))
# Task buttons (9 px margins make a 22 px icon in the 40 px cell): the cell is bare until hovered; the running app has its 2 px accent
# underline; the active one also a faint lift.
tasks = [
    frame("normal-", ACCENT, 9, fill_opacity=0.0, underline=None),
    frame("focus-", "#ffffff", 9, fill_opacity=0.0, underline=ACCENT),
    frame("hover-", "#ffffff", 9, fill_opacity=0.08),
    frame("attention-", "#c2502e", 9, fill_opacity=0.25, underline="#c2502e"),
    frame("minimized-", ACCENT, 9, fill_opacity=0.0, underline=ACCENT),
]
write("widgets/tasks.svg", svg("\n  ".join(tasks)))
