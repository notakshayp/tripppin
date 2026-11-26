#!/usr/bin/env python3
"""Writes the Kvantum "Tripppin" Qt widget style (os/files/usr/share/Kvantum/Tripppin).

Qt apps that are not ours (Dolphin, Konsole, System Settings, anything installed later) are
drawn by Kvantum (a separate GPL style plugin, loaded as a library by Qt, so it stays a
separate program for licence purposes). The theme is the "Bento" look: rounded tiles on a
graphite board, amber for the selected and active, no gradient. Buttons, fields, menus, tool
tips, list rows, sliders and scroll bars are rounded (RADII, in px; the slices are as big as
the radius, so Kvantum draws the arcs at 1:1); every element is a path or rectangle, so it is
vector at any scale. Run from the repository root:
python3 scripts/kvantum-theme.py
"""
import pathlib

OUT = pathlib.Path("os/files/usr/share/Kvantum/Tripppin")

BG = "#16181a"
BASE = "#111314"
RAISED = "#23272a"
RAISED2 = "#2e3337"
LINE = "#2e3337"
TEXT = "#f1f3f4"
MUTED = "#9aa3a8"
ACCENT = "#f2b84b"
GREEN = "#f2b84b"  # selected / pressed fill (amber; text on it is DARK)
GREEN2 = "#f7cc77"
DARK = "#16181a"
SLATE = "#3a3f43"
DIS = "#1c1f21"
NONE = "none"

STATES = ["normal", "focused", "pressed", "toggled", "disabled", "default", "toggled-focused", "toggled-pressed"]
POS = ["top", "bottom", "left", "right", "topleft", "topright", "bottomleft", "bottomright"]


def style(fill, line):
    return {"fill": fill, "line": line}


def box(fills):
    """fills: state -> (fill, line); missing states fall back to normal."""
    out = {}
    for s in STATES:
        out[s] = fills.get(s, fills["normal"])
    return out


def button(normal=(RAISED, RAISED)):
    return box({
        "normal": normal,
        "focused": (RAISED2, RAISED2),
        "pressed": (GREEN, GREEN),
        "toggled": (GREEN, GREEN),
        "toggled-focused": (GREEN2, GREEN2),
        "toggled-pressed": (GREEN, GREEN),
        "disabled": (DIS, DIS),
        "default": (RAISED, ACCENT),
    })


FRAMES = {
    "button": button(),
    "tbutton": box({"normal": (NONE, NONE), "focused": (RAISED, RAISED), "pressed": (GREEN, GREEN),
                    "toggled": (GREEN, GREEN), "toggled-focused": (GREEN2, GREEN2), "toggled-pressed": (GREEN, GREEN),
                    "disabled": (NONE, NONE), "default": (NONE, NONE)}),
    "lineedit": box({"normal": (BASE, LINE), "focused": (BASE, ACCENT), "disabled": (DIS, RAISED)}),
    "common": box({"normal": (NONE, LINE)}),
    "group": box({"normal": (NONE, LINE)}),
    "tabframe": box({"normal": (NONE, LINE)}),
    "toolbar": box({"normal": (BG, BG)}),
    "menu": box({"normal": (RAISED, LINE)}),
    "tooltip": box({"normal": (RAISED, LINE)}),
    "tab": box({"normal": (BG, LINE), "focused": (RAISED, LINE), "toggled": (GREEN, GREEN),
                "toggled-focused": (GREEN2, GREEN2), "pressed": (GREEN, GREEN), "disabled": (DIS, RAISED)}),
    "itemview": box({"normal": (NONE, NONE), "focused": (RAISED, RAISED), "pressed": (GREEN, GREEN),
                     "toggled": (GREEN, GREEN), "toggled-focused": (GREEN2, GREEN2), "toggled-pressed": (GREEN, GREEN),
                     "disabled": (NONE, NONE)}),
    "menuitem": box({"normal": (NONE, NONE), "focused": (GREEN, GREEN), "pressed": (GREEN, GREEN),
                     "toggled": (GREEN, GREEN)}),
    "menubaritem": box({"normal": (NONE, NONE), "focused": (RAISED, RAISED), "pressed": (GREEN, GREEN),
                        "toggled": (GREEN, GREEN)}),
    "header": box({"normal": (BG, LINE), "focused": (RAISED, LINE), "pressed": (GREEN, GREEN)}),
    "titlebar": box({"normal": (RAISED, RAISED), "focused": (GREEN, GREEN)}),
    "slider": box({"normal": (LINE, LINE), "toggled": (ACCENT, ACCENT), "disabled": (RAISED, RAISED)}),
    "progress": box({"normal": (RAISED, RAISED), "disabled": (RAISED, RAISED)}),
    "progress-pattern": box({"normal": (ACCENT, ACCENT), "disabled": (SLATE, SLATE)}),
    "slidercursor": box({"normal": (TEXT, TEXT), "focused": (ACCENT, ACCENT), "pressed": (ACCENT, ACCENT),
                         "disabled": (MUTED, MUTED)}),
    "scrollbargroove": box({"normal": (NONE, NONE)}),
    "scrollbarslider": box({"normal": (SLATE, SLATE), "focused": ("#4a5055", "#4a5055"), "pressed": (ACCENT, ACCENT),
                            "disabled": (RAISED, RAISED)}),
    "ss": box({"normal": (SLATE, SLATE), "focused": (SLATE, SLATE), "pressed": (ACCENT, ACCENT)}),
    "splitter": box({"normal": (NONE, NONE), "focused": (ACCENT, ACCENT), "pressed": (ACCENT, ACCENT)}),
    "dock": box({"normal": (BG, LINE)}),
}

ARROWS = {
    "up": "M1.5 6.5 L4.5 3 L7.5 6.5",
    "down": "M1.5 2.5 L4.5 6 L7.5 2.5",
    "left": "M6.5 1.5 L3 4.5 L6.5 7.5",
    "right": "M2.5 1.5 L6 4.5 L2.5 7.5",
}


def rect(x, y, w, h, fill, line=None, sw=1):
    fill_attr = fill if fill != NONE else "none"
    stroke = f' stroke="{line}" stroke-width="{sw}"' if line and line != NONE else ""
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill_attr}"{stroke}/>'


RADII = {"button": 10, "tbutton": 10, "lineedit": 10, "menu": 14, "tooltip": 12, "itemview": 8,
         "menuitem": 8, "menubaritem": 8, "scrollbarslider": 5, "ss": 5, "slider": 2, "progress": 2,
         "progress-pattern": 2, "slidercursor": 6}


def corner(r, which, fill, line):
    s_ = r - 0.5
    d = {
        "topleft": (f"M0 {r} A{r} {r} 0 0 1 {r} 0 V{r} Z", f"M.5 {r} A{s_} {s_} 0 0 1 {r} .5"),
        "topright": (f"M0 0 A{r} {r} 0 0 1 {r} {r} H0 Z", f"M0 .5 A{s_} {s_} 0 0 1 {r - .5} {r}"),
        "bottomleft": (f"M0 0 A{r} {r} 0 0 0 {r} {r} V0 Z", f"M.5 0 A{s_} {s_} 0 0 0 {r} {r - .5}"),
        "bottomright": (f"M{r} 0 A{r} {r} 0 0 1 0 {r} H{r} Z", f"M{r - .5} 0 A{s_} {s_} 0 0 1 0 {r - .5}"),
    }[which]
    out = f'<rect x="0" y="0" width="{r}" height="{r}" fill="none"/>'
    if fill != NONE:
        out += f'<path d="{d[0]}" fill="{fill}"/>'
    if line not in (NONE, fill):
        out += f'<path d="{d[1]}" fill="none" stroke="{line}" stroke-width="1"/>'
    return out


def rounded_group(base, state, fill, line, r):
    """Interior plus eight r-sized slices: arcs in the corners, the fill (and a hairline) on the edges."""
    p = f"{base}-{state}"
    f = fill if fill != NONE else "none"
    parts = [f'<g id="{p}"><rect x="0" y="0" width="6" height="6" fill="{f}"/></g>']
    for pos in POS:
        if pos in ("topleft", "topright", "bottomleft", "bottomright"):
            body = corner(r, pos, fill, line)
        else:
            w, h = (6, r) if pos in ("top", "bottom") else (r, 6)
            body = f'<rect x="0" y="0" width="{w}" height="{h}" fill="{f}"/>'
            if line not in (NONE, fill):
                ln = {"top": (0, 0, w, 1), "bottom": (0, h - 1, w, 1), "left": (0, 0, 1, h), "right": (w - 1, 0, 1, h)}[pos]
                body += f'<rect x="{ln[0]}" y="{ln[1]}" width="{ln[2]}" height="{ln[3]}" fill="{line}"/>'
        parts.append(f'<g id="{p}-{pos}">{body}</g>')
    return "\n".join(parts)


def frame_group(base, state, fill, line):
    if base in RADII:
        return rounded_group(base, state, fill, line, RADII[base])
    """The interior and the eight slices of one frame in one state."""
    p = f"{base}-{state}"
    # Interior: a 6 x 6 patch of the fill. Slices: the 1 px outline, stretched by Kvantum.
    parts = [f'<g id="{p}">{rect(0, 0, 6, 6, fill)}</g>']
    for pos in POS:
        parts.append(f'<g id="{p}-{pos}">{rect(0, 0, 1, 1, line if line != NONE else NONE)}</g>')
    return "\n".join(parts)


def glyphs():
    out = []
    for name, d in ARROWS.items():
        for state, colour in [("normal", TEXT), ("focused", TEXT), ("pressed", TEXT), ("toggled", TEXT), ("disabled", "#5a6268")]:
            out.append(
                f'<g id="arrow-{name}-{state}"><rect x="0" y="0" width="9" height="9" fill="none"/>'
                f'<path d="{d}" fill="none" stroke="{colour}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></g>'
            )
    # Plus and minus for the +/- of spin boxes.
    for name, d in [("plus", "M1 4.5 H8 M4.5 1 V8"), ("minus", "M1 4.5 H8")]:
        for state in ["normal", "pressed"]:
            out.append(
                f'<g id="arrow-{name}-{state}"><rect x="0" y="0" width="9" height="9" fill="none"/>'
                f'<path d="{d}" fill="none" stroke="{TEXT}" stroke-width="1.5"/></g>'
            )
    # Check boxes and radio buttons (square: the Tiles look has no circles).
    for kind in ("checkbox", "radio"):
        for state, line, fill in [("normal", MUTED, NONE), ("focused", ACCENT, NONE), ("pressed", ACCENT, RAISED),
                                  ("disabled", "#5a6268", NONE)]:
            box_el = (f'<circle cx="8" cy="8" r="7" fill="{fill}" stroke="{line}" stroke-width="1.5"/>' if kind == "radio" else f'<rect x="1" y="1" width="14" height="14" rx="4" fill="{fill}" stroke="{line}" stroke-width="1.5"/>')
            out.append(f'<g id="{kind}-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{box_el}</g>')
            if kind == "checkbox":
                mark = f'<path d="M4 8.5 L7 11.5 L12 4.5" fill="none" stroke="{DARK}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>'
                tri = f'<path d="M4.5 8 H11.5" fill="none" stroke="{DARK}" stroke-width="2" stroke-linecap="round"/>'
                c_ = GREEN if state != "disabled" else RAISED
                solid = f'<rect x="1" y="1" width="14" height="14" rx="4" fill="{c_}"/>'
                out.append(f'<g id="checkbox-checked-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{solid}{mark}</g>')
                out.append(f'<g id="checkbox-tristate-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{solid}{tri}</g>')
            else:
                dot = f'<circle cx="8" cy="8" r="3" fill="{ACCENT if state != "disabled" else "#5a6268"}"/>'
                out.append(f'<g id="radio-checked-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{box_el}{dot}</g>')
    # Grips, tab tear, header separator, menu indicator: small and quiet.
    out.append(f'<g id="grip-normal"><rect x="0" y="0" width="13" height="13" fill="none"/>{rect(4, 4, 5, 1, MUTED)}{rect(4, 8, 5, 1, MUTED)}</g>')
    out.append(f'<g id="grip-focused"><rect x="0" y="0" width="13" height="13" fill="none"/>{rect(4, 4, 5, 1, TEXT)}{rect(4, 8, 5, 1, TEXT)}</g>')
    out.append(f'<g id="grip-pressed"><rect x="0" y="0" width="13" height="13" fill="none"/>{rect(4, 4, 5, 1, TEXT)}{rect(4, 8, 5, 1, TEXT)}</g>')
    out.append(f'<g id="splitter-grip-normal"><rect x="0" y="0" width="16" height="16" fill="none"/>{rect(7, 4, 2, 8, LINE)}</g>')
    out.append(f'<g id="splitter-grip-focused"><rect x="0" y="0" width="16" height="16" fill="none"/>{rect(7, 4, 2, 8, ACCENT)}</g>')
    out.append(f'<g id="resize-grip-normal"><rect x="0" y="0" width="13" height="13" fill="none"/>{rect(8, 11, 4, 1, MUTED)}{rect(11, 8, 1, 4, MUTED)}</g>')
    out.append(f'<g id="toolbar-handle"><rect x="0" y="0" width="5" height="16" fill="none"/>{rect(2, 2, 1, 12, LINE)}</g>')
    out.append(f'<g id="header-separator"><rect x="0" y="0" width="1" height="16" fill="{LINE}"/></g>')
    out.append(f'<g id="tab-tear"><rect x="0" y="0" width="6" height="16" fill="none"/>{rect(2, 0, 1, 16, LINE)}</g>')
    return "\n".join(out)


def svg():
    parts = []
    for base, states in FRAMES.items():
        for state, (fill, line) in states.items():
            parts.append(frame_group(base, state, fill, line))
    parts.append(glyphs())
    body = "\n".join(parts)
    return f'<?xml version="1.0" encoding="UTF-8"?>\n<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">\n{body}\n</svg>\n'


KVCONFIG = f"""[%General]
author=Tripppin
comment=Bento: rounded tiles on graphite, amber accent (generated by scripts/kvantum-theme.py)
x11drag=all
alt_mnemonic=true
left_tabs=false
attach_active_tab=false
group_toolbar_buttons=false
spread_progressbar=true
progressbar_thickness=6
composite=true
menu_shadow_depth=8
tooltip_shadow_depth=6
translucent_windows=false
blurring=false
popup_blurring=false
splitter_width=6
scroll_width=10
scroll_min_extent=40
scroll_arrows=false
combo_as_lineedit=true
inline_spin_indicators=true
slider_width=6
slider_handle_width=14
slider_handle_length=14
check_size=16
toolbar_icon_size=16
animate_states=true
transient_groove=false
transient_scrollbar=true
reduce_window_opacity=0
reduce_menu_opacity=0
layout_spacing=4
layout_margin=6
submenu_overlap=0
scrollbar_in_view=true
tree_branch_line=false

[GeneralColors]
window.color={BG}
base.color={BASE}
alt.base.color={BG}
button.color={RAISED}
light.color={RAISED2}
mid.light.color={RAISED}
dark.color={BASE}
mid.color={LINE}
highlight.color={GREEN}
inactive.highlight.color={SLATE}
text.color={TEXT}
window.text.color={TEXT}
button.text.color={TEXT}
disabled.text.color=#4b555c
tooltip.text.color={TEXT}
highlight.text.color={DARK}
link.color={ACCENT}
link.visited.color={MUTED}

[Hacks]
respect_darkness=true
transparent_ktitle_label=true
transparent_dolphin_view=false
transparent_pcmanfm_sidepane=false
blur_translucent=false
no_selection_tint=true

[PanelButtonCommand]
frame=true
frame.element=button
interior=true
interior.element=button
indicator.size=9
text.normal.color={TEXT}
text.focus.color={TEXT}
text.press.color={DARK}
text.toggle.color={DARK}
text.shadow=0
text.margin=1
text.iconspacing=4
indicator.element=arrow
text.margin.top=4
text.margin.bottom=4
text.margin.left=8
text.margin.right=8
frame.top=10
frame.bottom=10
frame.left=10
frame.right=10

[PanelButtonTool]
inherits=PanelButtonCommand
interior.element=tbutton
frame.element=tbutton
text.margin.top=3
text.margin.bottom=3
text.margin.left=4
text.margin.right=4
frame.top=10
frame.bottom=10
frame.left=10
frame.right=10

[Dock]
inherits=PanelButtonCommand
frame=true
interior=true
frame.element=dock
interior.element=dock

[DockTitle]
inherits=PanelButtonCommand
frame=false
interior=false
text.focus.color={TEXT}
text.bold=true

[IndicatorSpinBox]
inherits=PanelButtonCommand
indicator.element=arrow
indicator.size=9

[RadioButton]
inherits=PanelButtonCommand
frame=false
interior.element=radio

[CheckBox]
inherits=PanelButtonCommand
frame=false
interior.element=checkbox

[GenericFrame]
inherits=PanelButtonCommand
frame=true
interior=false
frame.element=common
interior.element=common

[LineEdit]
inherits=PanelButtonCommand
frame.element=lineedit
interior.element=lineedit
text.margin.left=2
text.margin.right=2
text.margin.top=0
text.margin.bottom=0
frame.top=10
frame.bottom=10
frame.left=10
frame.right=10

[DropDownButton]
inherits=PanelButtonCommand
indicator.element=arrow-down

[IndicatorArrow]
indicator.element=arrow
indicator.size=9

[ToolboxTab]
inherits=PanelButtonCommand

[Tab]
inherits=PanelButtonCommand
interior.element=tab
text.margin.left=10
text.margin.right=10
text.margin.top=5
text.margin.bottom=5
frame.element=tab
indicator.element=tab
min_width=+2
min_height=2.0font

[TabFrame]
inherits=PanelButtonCommand
frame.element=tabframe
interior=false

[TreeExpander]
inherits=PanelButtonCommand
frame=false
interior=false
indicator.size=9

[HeaderSection]
inherits=PanelButtonCommand
frame.element=header
interior.element=header
text.margin.top=3
text.margin.bottom=3

[SizeGrip]
inherits=PanelButtonCommand
frame=false
interior=false
indicator.element=resize-grip
indicator.size=13

[Toolbar]
inherits=PanelButtonCommand
indicator.element=toolbar
indicator.size=5
text.margin=0
frame.element=toolbar
interior=false

[Slider]
inherits=PanelButtonCommand
frame.element=slider
interior.element=slider

frame.top=2
frame.bottom=2
frame.left=2
frame.right=2

[SliderCursor]
inherits=PanelButtonCommand
frame=false
interior.element=slidercursor
frame.top=6
frame.bottom=6
frame.left=6
frame.right=6

[Progressbar]
inherits=PanelButtonCommand
frame.element=progress
interior.element=progress
text.margin=0
text.focus.color={TEXT}
text.press.color={TEXT}
text.toggle.color={TEXT}
text.bold=false
frame.top=2
frame.bottom=2
frame.left=2
frame.right=2

[ProgressbarContents]
inherits=PanelButtonCommand
frame=true
frame.element=progress-pattern
interior.element=progress-pattern

frame.top=2
frame.bottom=2
frame.left=2
frame.right=2

[ItemView]
inherits=PanelButtonCommand
text.margin=1
frame.element=itemview
interior.element=itemview
text.normal.color={TEXT}
text.focus.color={TEXT}
text.press.color={TEXT}
text.toggle.color={TEXT}
frame.top=8
frame.bottom=8
frame.left=8
frame.right=8

[Splitter]
inherits=PanelButtonCommand
interior.element=splitter
frame.element=splitter
frame.top=0
frame.bottom=0
frame.left=0
frame.right=0
indicator.element=splitter-grip
indicator.size=16

[Scrollbar]
inherits=PanelButtonCommand
indicator.size=9

[ScrollbarSlider]
inherits=PanelButtonCommand
frame.element=scrollbarslider
interior.element=scrollbarslider
indicator.element=grip
indicator.size=13
frame.top=5
frame.bottom=5
frame.left=5
frame.right=5

[ScrollbarTransientSlider]
frame=true
frame.element=ss
interior=false

frame.top=5
frame.bottom=5
frame.left=5
frame.right=5

[ScrollbarGroove]
inherits=PanelButtonCommand
interior.element=scrollbargroove
frame.element=scrollbargroove
frame.top=0
frame.bottom=0
frame.left=0
frame.right=0

[MenuItem]
inherits=PanelButtonCommand
frame=true
frame.element=menuitem
interior.element=menuitem
indicator.element=menuitem
text.focus.color={DARK}
text.margin.top=4
text.margin.bottom=4
text.margin.left=8
text.margin.right=8
frame.top=8
frame.bottom=8
frame.left=8
frame.right=8

[MenuBarItem]
inherits=PanelButtonCommand
interior.element=menubaritem
frame.element=menubaritem
text.margin.left=8
text.margin.right=8
frame.top=8
frame.bottom=8
frame.left=8
frame.right=8

[TitleBar]
inherits=PanelButtonCommand
frame=false
interior.element=titlebar
indicator.size=12
text.normal.color={MUTED}
text.focus.color={TEXT}
text.bold=true
text.italic=false

[ComboBox]
inherits=PanelButtonCommand

[Menu]
inherits=PanelButtonCommand
frame.element=menu
interior.element=menu
frame.top=14
frame.bottom=14
frame.left=14
frame.right=14

[GroupBox]
inherits=GenericFrame
frame=true
frame.element=group
text.margin=0

[TabBarFrame]
inherits=GenericFrame
frame=false
interior=false

[ToolTip]
inherits=GenericFrame
interior=true
text.margin=2
interior.element=tooltip
frame.element=tooltip
frame.top=12
frame.bottom=12
frame.left=12
frame.right=12

[StatusBar]
inherits=GenericFrame
frame=false
interior=false

[Window]
interior=false
"""


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "Tripppin.svg").write_text(svg())
    (OUT / "Tripppin.kvconfig").write_text(KVCONFIG)


main()
