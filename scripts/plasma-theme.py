#!/usr/bin/env python3
"""Writes the Plasma style's frame SVGs (os/files/usr/share/plasma/desktoptheme/tripppin-bento).

The "Bento" look: rounded tiles on a graphite board. Plasma draws panels, dialogs, tooltips and
the task buttons from nine-patch SVGs (corners, edges, centre and `hint-*-margin` rectangles);
ours are vector paths in the design tokens: rail #1c1f21 with a 1 px #2a2e31 edge, tiles
#23272a, floating boards #1f2225 with a #2e3337 hairline. Everything else falls back to
Breeze's. Run from the repository root: python3 scripts/plasma-theme.py
"""
import pathlib

OUT = pathlib.Path("os/files/usr/share/plasma/desktoptheme/tripppin-bento")

RAIL = "#1c1f21"
RAIL_EDGE = "#2a2e31"
TILE = "#23272a"
RAISED = "#1f2225"
LINE = "#2e3337"
AMBER = "#f2b84b"
RED = "#ff6b6b"


def corner(r, x0, y0, which, fill, border=None, opacity=1.0):
    """One rounded corner cell (r x r at x0,y0): the fill, plus an optional 1 px hairline arc."""
    x1, y1 = x0 + r, y0 + r
    s = r - 0.5
    d = {
        "tl": (f"M{x0} {y1} A{r} {r} 0 0 1 {x1} {y0} V{y1} Z", f"M{x0 + .5} {y1} A{s} {s} 0 0 1 {x1} {y0 + .5}"),
        "tr": (f"M{x0} {y0} A{r} {r} 0 0 1 {x1} {y1} H{x0} Z", f"M{x0} {y0 + .5} A{s} {s} 0 0 1 {x1 - .5} {y1}"),
        "bl": (f"M{x0} {y0} A{r} {r} 0 0 0 {x1} {y1} V{y0} Z", f"M{x0 + .5} {y0} A{s} {s} 0 0 0 {x1} {y1 - .5}"),
        "br": (f"M{x1} {y0} A{r} {r} 0 0 1 {x0} {y1} H{x1} Z", f"M{x1 - .5} {y0} A{s} {s} 0 0 1 {x0} {y1 - .5}"),
    }[which]
    out = f'<path d="{d[0]}" fill="{fill}" fill-opacity="{opacity}"/>'
    if border:
        out += f'<path d="{d[1]}" fill="none" stroke="{border}" stroke-width="1"/>'
    return out


def hints(p, left, right, top, bottom):
    out = []
    for side, v in (("left", left), ("right", right), ("top", top), ("bottom", bottom)):
        w = v if side in ("left", "right") else 1
        h = v if side in ("top", "bottom") else 1
        out.append(f'<rect id="{p}hint-{side}-margin" x="0" y="0" width="{max(w, 0.01)}" height="{max(h, 0.01)}" fill="none"/>')
    out.append(f'<rect id="{p}hint-stretch-borders" x="0" y="0" width="1" height="1" fill="none"/>')
    return out


def frame(prefix, fill, r, margin, border=None):
    """A rounded nine-patch on a 3r x 3r grid: r x r corners and edges, an r x r centre."""
    p, parts = prefix, []
    for name, (cx, cy, k) in {"topleft": (0, 0, "tl"), "topright": (2, 0, "tr"),
                              "bottomleft": (0, 2, "bl"), "bottomright": (2, 2, "br")}.items():
        parts.append(f'<g id="{p}{name}">{corner(r, cx * r, cy * r, k, fill, border)}</g>')
    for name, (cx, cy) in {"top": (1, 0), "bottom": (1, 2), "left": (0, 1), "right": (2, 1), "center": (1, 1)}.items():
        x, y = cx * r, cy * r
        body = f'<rect x="{x}" y="{y}" width="{r}" height="{r}" fill="{fill}"/>'
        if border:
            line = {"top": (x, y, r, 1), "bottom": (x, y + r - 1, r, 1),
                    "left": (x, y, 1, r), "right": (x + r - 1, y, 1, r)}.get(name)
            if line:
                body += f'<rect x="{line[0]}" y="{line[1]}" width="{line[2]}" height="{line[3]}" fill="{border}"/>'
        parts.append(f'<g id="{p}{name}">{body}</g>')
    parts += hints(p, margin, margin, margin, margin)
    return "\n  ".join(parts), (3 * r, 3 * r)


def rail_frame(prefix):
    """The left rail: flat #1c1f21 on a 6 x 6 grid with a 1 px #2a2e31 line on its right edge."""
    p, parts = prefix, []
    for name, (x, y) in {"topleft": (0, 0), "top": (2, 0), "topright": (4, 0), "left": (0, 2), "center": (2, 2),
                         "right": (4, 2), "bottomleft": (0, 4), "bottom": (2, 4), "bottomright": (4, 4)}.items():
        edge = f'<rect x="{x + 1}" y="{y}" width="1" height="2" fill="{RAIL_EDGE}"/>' if x == 4 else ""
        parts.append(f'<g id="{p}{name}"><rect x="{x}" y="{y}" width="2" height="2" fill="{RAIL}"/>{edge}</g>')
    parts += hints(p, 0, 0, 0, 0)
    return "\n  ".join(parts), (6, 6)


def task_frame(prefix, fill, opacity, bar=False):
    """A task button in the rail: a rounded 12 px tile inset 13 px from each side (a 42 px icon in
    the 68 px rail) with 4 px transparent padding above and below; a running app also gets a 3 px
    amber bar on the rail's left edge. Columns 25 | 8 | 25, rows 16 | 8 | 16."""
    p, r, parts = prefix, 12, []
    cols = [(0, 25), (25, 8), (33, 25)]
    rows = [(0, 16), (16, 8), (24, 16)]
    names = [["topleft", "top", "topright"], ["left", "center", "right"], ["bottomleft", "bottom", "bottomright"]]
    for j, (y, h) in enumerate(rows):
        for i, (x, w) in enumerate(cols):
            body = ""
            if opacity > 0:
                fx = x + (13 if i == 0 else 0)
                fw = w - (13 if i in (0, 2) else 0)
                fy = y + (4 if j == 0 else 0)
                fh = h - (4 if j in (0, 2) else 0)
                if i == 1 or j == 1:
                    body = f'<rect x="{fx}" y="{fy}" width="{fw}" height="{fh}" fill="{fill}" fill-opacity="{opacity}"/>'
                else:
                    k = {(0, 0): "tl", (0, 2): "tr", (2, 0): "bl", (2, 2): "br"}[(j, i)]
                    body = corner(r, fx, fy, k, fill, None, opacity)
            if bar and i == 0 and j == 1:
                body += f'<rect x="0" y="0" width="3" height="{h}" rx="1.5" fill="{AMBER}"/>'
            if not body:
                body = f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none"/>'
            parts.append(f'<g id="{p}{names[j][i]}">{body}</g>')
    parts += hints(p, 13, 13, 4, 4)
    return "\n  ".join(parts), (58, 40)


def svg(body, size):
    w, h = size
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
  {body}
</svg>
"""


def write(rel, pair):
    path = OUT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(svg(*pair))


# The rail (a left-docked panel): flat, edge line on the right.
write("widgets/panel-background.svg", (rail_frame("")[0] + "\n  " + rail_frame("floating-")[0], (6, 6)))
# Pop-ups (tray, calendar, volume, OSD) are floating boards: 24 px corners, #1f2225, hairline.
write("dialogs/background.svg", frame("", RAISED, 24, 16, border=LINE))
# Applet backgrounds are tiles: 20 px.
write("widgets/background.svg", frame("", TILE, 20, 12))
write("widgets/tooltip.svg", frame("", RAISED, 12, 8, border=LINE))
# Task buttons: bare until hovered; the running app has the amber bar; the active one also a lift.
tasks = [
    task_frame("normal-", "#ffffff", 0.0),
    task_frame("focus-", "#ffffff", 0.08, bar=True),
    task_frame("hover-", "#ffffff", 0.10),
    task_frame("attention-", RED, 0.25, bar=True),
    task_frame("minimized-", "#ffffff", 0.0, bar=True),
]
write("widgets/tasks.svg", ("\n  ".join(t[0] for t in tasks), tasks[0][1]))
