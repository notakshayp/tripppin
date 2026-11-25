#!/usr/bin/env python3
"""Writes the Kvantum "Tripppin" Qt widget style (os/files/usr/share/Kvantum/Tripppin).

Qt apps that are not ours (Dolphin, Konsole, System Settings, anything installed later) are
drawn by Kvantum (a separate GPL style plugin, loaded as a library by Qt, so it stays a
separate program for licence purposes). The theme is the "Tiles" look: flat, square, solid
colour, 1 px outlines, no shadow, no gradient. Every element is a plain rectangle or a
stroked line, so it is vector at any scale. Run from the repository root:
python3 scripts/kvantum-theme.py
"""
import pathlib

OUT = pathlib.Path("os/files/usr/share/Kvantum/Tripppin")

BG = "#101418"
BASE = "#0b0e11"
RAISED = "#1a2025"
RAISED2 = "#222a30"
LINE = "#2a3238"
TEXT = "#ffffff"
MUTED = "#9aa4ab"
ACCENT = "#3fae7a"
GREEN = "#1f6b45"
GREEN2 = "#2a7b54"
SLATE = "#3a4a5a"
DIS = "#14191d"
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


def button(normal=(RAISED, LINE)):
    return box({
        "normal": normal,
        "focused": (RAISED2, ACCENT),
        "pressed": (GREEN, GREEN),
        "toggled": (GREEN, GREEN),
        "toggled-focused": (GREEN2, ACCENT),
        "toggled-pressed": (GREEN, GREEN),
        "disabled": (DIS, RAISED),
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
    "scrollbarslider": box({"normal": (SLATE, SLATE), "focused": ("#4b5d70", "#4b5d70"), "pressed": (ACCENT, ACCENT),
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


def frame_group(base, state, fill, line):
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
        for state, colour in [("normal", TEXT), ("focused", TEXT), ("pressed", TEXT), ("toggled", TEXT), ("disabled", "#4b555c")]:
            out.append(
                f'<g id="arrow-{name}-{state}"><rect x="0" y="0" width="9" height="9" fill="none"/>'
                f'<path d="{d}" fill="none" stroke="{colour}" stroke-width="1.5" stroke-linecap="square"/></g>'
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
                                  ("disabled", "#4b555c", NONE)]:
            box_el = rect(1, 1, 14, 14, fill, line, 1.5)
            out.append(f'<g id="{kind}-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{box_el}</g>')
            if kind == "checkbox":
                mark = f'<path d="M4 8.5 L7 11.5 L12 4.5" fill="none" stroke="{TEXT}" stroke-width="1.8"/>'
                tri = f'<path d="M4.5 8 H11.5" fill="none" stroke="{TEXT}" stroke-width="1.8"/>'
                solid = rect(1, 1, 14, 14, GREEN if state != "disabled" else RAISED, GREEN if state != "disabled" else RAISED)
                out.append(f'<g id="checkbox-checked-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{solid}{mark}</g>')
                out.append(f'<g id="checkbox-tristate-{state}"><rect x="0" y="0" width="16" height="16" fill="none"/>{solid}{tri}</g>')
            else:
                dot = rect(5, 5, 6, 6, TEXT if state != "disabled" else "#4b555c")
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
comment=Tiles: flat, square, solid colour (generated by scripts/kvantum-theme.py)
x11drag=all
alt_mnemonic=true
left_tabs=false
attach_active_tab=false
group_toolbar_buttons=false
spread_progressbar=true
progressbar_thickness=4
composite=true
menu_shadow_depth=0
tooltip_shadow_depth=0
translucent_windows=false
blurring=false
popup_blurring=false
splitter_width=6
scroll_width=10
scroll_min_extent=40
scroll_arrows=false
combo_as_lineedit=true
inline_spin_indicators=true
slider_width=4
slider_handle_width=12
slider_handle_length=12
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
highlight.text.color={TEXT}
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
frame.top=1
frame.bottom=1
frame.left=1
frame.right=1
interior=true
interior.element=button
indicator.size=9
text.normal.color={TEXT}
text.focus.color={TEXT}
text.press.color={TEXT}
text.toggle.color={TEXT}
text.shadow=0
text.margin=1
text.iconspacing=4
indicator.element=arrow
text.margin.top=4
text.margin.bottom=4
text.margin.left=8
text.margin.right=8

[PanelButtonTool]
inherits=PanelButtonCommand
interior.element=tbutton
frame.element=tbutton
text.margin.top=3
text.margin.bottom=3
text.margin.left=4
text.margin.right=4

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
text.margin.left=4
text.margin.right=4
text.margin.top=3
text.margin.bottom=3

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
frame.top=0
frame.bottom=0
frame.left=0
frame.right=0

[SliderCursor]
inherits=PanelButtonCommand
frame=false
interior.element=slidercursor

[Progressbar]
inherits=PanelButtonCommand
frame.element=progress
interior.element=progress
text.margin=0
text.focus.color={TEXT}
text.press.color={TEXT}
text.toggle.color={TEXT}
text.bold=false

[ProgressbarContents]
inherits=PanelButtonCommand
frame=true
frame.element=progress-pattern
interior.element=progress-pattern
frame.top=0
frame.bottom=0
frame.left=0
frame.right=0

[ItemView]
inherits=PanelButtonCommand
text.margin=1
frame.element=itemview
interior.element=itemview
text.normal.color={TEXT}
text.focus.color={TEXT}
text.press.color={TEXT}
text.toggle.color={TEXT}

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
frame.top=0
frame.bottom=0
frame.left=0
frame.right=0
indicator.element=grip
indicator.size=13

[ScrollbarTransientSlider]
frame=true
frame.element=ss
interior=false
frame.top=0
frame.bottom=0
frame.left=0
frame.right=0

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
text.focus.color={TEXT}
text.margin.top=4
text.margin.bottom=4
text.margin.left=8
text.margin.right=8

[MenuBarItem]
inherits=PanelButtonCommand
interior.element=menubaritem
frame.element=menubaritem
text.margin.left=8
text.margin.right=8

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
