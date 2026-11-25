#!/usr/bin/env python3
"""Writes the Plasma style's frame SVGs (os/files/usr/share/plasma/desktoptheme/tripppin).

Plasma draws panels, dialogs and tooltips from nine-patch SVGs (corners, edges, centre and
`hint-*-margin` rectangles). Ours are plain rounded rectangles in the colour scheme's
background with a neutral hairline: no glow, no gradients. Everything else in the theme falls
back to Breeze's. Run from the repository root: python3 scripts/plasma-theme.py
"""
import pathlib

OUT = pathlib.Path("os/files/usr/share/plasma/desktoptheme/tripppin")


def frame(prefixes, r, margin, opacity, hairline=0.14):
    w = 4 * r
    a, b = r, w - r
    h = r - 0.5
    parts = []
    for p in prefixes:
        fill = f'class="ColorScheme-Background" fill="currentColor" fill-opacity="{opacity}" stroke="none"'
        line = f'class="ColorScheme-Text" fill="currentColor" fill-opacity="{hairline}" stroke="none"'
        arc = f'class="ColorScheme-Text" fill="none" stroke="currentColor" stroke-opacity="{hairline}" stroke-width="1"'
        parts += [
            f'<g id="{p}topleft"><path {fill} d="M{a},0 A{r},{r} 0 0 0 0,{a} L{a},{a}Z"/><path {arc} d="M{a},0.5 A{h},{h} 0 0 0 0.5,{a}"/></g>',
            f'<g id="{p}topright"><path {fill} d="M{b},0 A{r},{r} 0 0 1 {w},{a} L{b},{a}Z"/><path {arc} d="M{b},0.5 A{h},{h} 0 0 1 {w - 0.5},{a}"/></g>',
            f'<g id="{p}bottomleft"><path {fill} d="M0,{b} A{r},{r} 0 0 0 {a},{w} L{a},{b}Z"/><path {arc} d="M0.5,{b} A{h},{h} 0 0 0 {a},{w - 0.5}"/></g>',
            f'<g id="{p}bottomright"><path {fill} d="M{w},{b} A{r},{r} 0 0 1 {b},{w} L{b},{b}Z"/><path {arc} d="M{w - 0.5},{b} A{h},{h} 0 0 1 {b},{w - 0.5}"/></g>',
            f'<g id="{p}top"><rect {fill} x="{a}" y="0" width="{b - a}" height="{a}"/><rect {line} x="{a}" y="0" width="{b - a}" height="1"/></g>',
            f'<g id="{p}bottom"><rect {fill} x="{a}" y="{b}" width="{b - a}" height="{a}"/><rect {line} x="{a}" y="{w - 1}" width="{b - a}" height="1"/></g>',
            f'<g id="{p}left"><rect {fill} x="0" y="{a}" width="{a}" height="{b - a}"/><rect {line} x="0" y="{a}" width="1" height="{b - a}"/></g>',
            f'<g id="{p}right"><rect {fill} x="{b}" y="{a}" width="{a}" height="{b - a}"/><rect {line} x="{w - 1}" y="{a}" width="1" height="{b - a}"/></g>',
            f'<rect id="{p}center" {fill} x="{a}" y="{a}" width="{b - a}" height="{b - a}"/>',
            f'<rect id="{p}hint-top-margin" x="0" y="0" width="1" height="{margin}" fill="none"/>',
            f'<rect id="{p}hint-bottom-margin" x="0" y="0" width="1" height="{margin}" fill="none"/>',
            f'<rect id="{p}hint-left-margin" x="0" y="0" width="{margin}" height="1" fill="none"/>',
            f'<rect id="{p}hint-right-margin" x="0" y="0" width="{margin}" height="1" fill="none"/>',
            f'<rect id="{p}hint-stretch-borders" x="0" y="0" width="1" height="1" fill="none"/>',
        ]
    body = "\n  ".join(parts)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{w}" viewBox="0 0 {w} {w}">
  <defs><style type="text/css" id="current-color-scheme">
    .ColorScheme-Text {{ color:#eef2ee; }}
    .ColorScheme-Background {{ color:#141a18; }}
  </style></defs>
  {body}
</svg>
"""


def write(rel, text):
    path = OUT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


# A floating panel with a softly rounded edge; dialogs a little rounder than tooltips.
write("widgets/panel-background.svg", frame(["", "floating-"], 14, 6, 0.94))
write("dialogs/background.svg", frame([""], 14, 10, 0.97))
write("widgets/tooltip.svg", frame([""], 10, 6, 0.98))
