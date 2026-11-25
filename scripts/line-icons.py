#!/usr/bin/env python3
"""Writes the "Tripppin Line" icon theme (os/files/usr/share/icons/TripppinLine).

Mock section 08: real app icons stay as they are (Papirus, through the Tripppin theme this one
inherits), while the system and toolbar glyphs are thin white line icons: 24 x 24 box, 1.5
stroke, round caps and joins. Plasma and Qt apps use this theme, so the tray, quick actions and pop-ups draw them. Every file is one scalable SVG that
takes the colour scheme's text colour, so it is crisp at any size and pixel density.
Run from the repository root: python3 scripts/line-icons.py
"""
import pathlib

OUT = pathlib.Path("os/files/usr/share/icons/TripppinLine")

# Paths from the Tiles mock, section 08, plus a few in the same hand for toolbars.
P = {
    "wifi": "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01",
    "wifi-weak": "M8.5 16a5 5 0 0 1 7 0M12 19.5h.01",
    "wifi-ok": "M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01",
    "wifi-off": "M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01M3 3l18 18",
    "wired": "M3 7h18v8H3zM7 15v4M12 15v4M17 15v4M7 7v3M12 7v3M17 7v3",
    "volume": "M4 9h4l5-4v14l-5-4H4zM16 9a4 4 0 0 1 0 6M19 6a8 8 0 0 1 0 12",
    "volume-medium": "M4 9h4l5-4v14l-5-4H4zM16 9a4 4 0 0 1 0 6",
    "volume-low": "M4 9h4l5-4v14l-5-4H4z",
    "volume-muted": "M4 9h4l5-4v14l-5-4H4zM17 9l5 6M22 9l-5 6",
    "mic": "M9 4a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0zM5 11a7 7 0 0 0 14 0M12 18v3",
    "mic-muted": "M9 4a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0zM5 11a7 7 0 0 0 14 0M12 18v3M3 3l18 18",
    "bluetooth": "M7 7l10 10-5 4V3l5 4L7 17",
    "bell": "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0",
    "bell-off": "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0M3 3l18 18",
    "search": "M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14M20 20l-4-4",
    "lock": "M5 11h14v10H5zM8 11V7a4 4 0 0 1 8 0v4",
    "display": "M3 4h18v12H3zM8 20h8M12 16v4",
    "user": "M12 4a4 4 0 1 0 0 8a4 4 0 1 0 0-8M4 21a8 8 0 0 1 16 0",
    "shield": "M12 3l8 3v6c0 5-4 8-8 9-4-1-8-4-8-9V6z",
    "power": "M12 3v9M6.3 6.3a8 8 0 1 0 11.4 0",
    "download": "M12 4v12M7 11l5 5 5-5M5 20h14",
    "clock": "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 7v5l3 2",
    "doc": "M6 3h8l4 4v14H6zM14 3v4h4",
    "print": "M6 9V3h12v6M6 18H4v-7h16v7h-2M6 14h12v7H6z",
    "grid": "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
    "moon": "M20 14A8 8 0 1 1 10 4a6 6 0 0 0 10 10z",
    "left": "M15 5l-7 7 7 7",
    "right": "M9 5l7 7-7 7",
    "up": "M5 15l7-7 7 7",
    "down": "M5 9l7 7 7-7",
    "close": "M6 6l12 12M18 6L6 18",
    "add": "M12 5v14M5 12h14",
    "remove": "M5 12h14",
    "refresh": "M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6",
    "copy": "M9 9h11v11H9zM5 15H4V4h11v1",
    "paste": "M8 4h8v3H8zM6 6H5v15h14V6h-1",
    "tab-new": "M4 6h16v14H4zM12 10v6M9 13h6",
    "menu": "M4 7h16M4 12h16M4 17h16",
    "split": "M3 5h18v14H3zM12 5v14",
    "folder": "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    "trash": "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13",
    "settings": "M4 6h9M17 6h3M15 4v4M4 12h3M11 12h9M9 10v4M4 18h11M19 18h1M17 16v4",
    "calendar": "M4 6h16v14H4zM4 10h16M8 3v4M16 3v4",
    "info": "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 11v6M12 7.5h.01",
    "warning": "M12 3l10 18H2zM12 10v5M12 18h.01",
    "error": "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 8v5M12 16h.01",
    "check": "M5 12l5 5 9-10",
    "stop": "M6 6h12v12H6z",
    "play": "M7 4l13 8-13 8z",
    "pause": "M7 5v14M17 5v14",
    "next": "M6 5l10 7-10 7zM19 5v14",
    "previous": "M18 5L8 12l10 7zM5 5v14",
    "keyboard": "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10",
    "key": "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10",
    "star": "M12 3l2.7 6 6.3.6-4.8 4.2 1.5 6.2L12 16.8 6.3 20l1.5-6.2L3 9.6 9.3 9z",
    "home": "M4 11l8-7 8 7M6 10v10h12V10",
    "edit": "M4 20h4L19 9l-4-4L4 16zM13 7l4 4",
    "save": "M4 4h13l3 3v13H4zM8 4v5h8V4M8 20v-6h8v6",
    "undo": "M9 6L4 11l5 5M4 11h10a6 6 0 0 1 0 12",
    "zoom-in": "M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14M20 20l-4-4M11 8v6M8 11h6",
    "zoom-out": "M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14M20 20l-4-4M8 11h6",
    "fullscreen": "M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5",
    "eye": "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6",
    "link": "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
    "terminal": "M4 6l5 6-5 6M12 18h8",
    "airplane": "M12 3v6l9 5v2l-9-2v5l3 2v1l-4-1-4 1v-1l3-2v-5l-9 2v-2l9-5z",
}


def battery(level, charging):
    bars = "".join(f"M{6 + 3 * i} 10v4" for i in range(3) if level > (i * 33))
    bolt = "M11 8l-2 4h4l-2 4" if charging else ""
    return "M3 7h15v10H3zM21 10v4" + (bars if not charging else bolt + bars[:0])


def svg(path):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24">'
        '<style type="text/css" id="current-color-scheme">.ColorScheme-Text{color:#ffffff;}</style>'
        '<g class="ColorScheme-Text" fill="none" stroke="currentColor" stroke-width="1.5" '
        f'stroke-linecap="round" stroke-linejoin="round"><path d="{path}"/></g></svg>\n'
    )


# freedesktop and Breeze names -> glyph. Each is written plain and with -symbolic.
NAMES = {
    "status": {
        "network-wireless": "wifi", "network-wireless-connected": "wifi", "network-wireless-signal-excellent": "wifi",
        "network-wireless-signal-good": "wifi", "network-wireless-signal-ok": "wifi-ok",
        "network-wireless-signal-weak": "wifi-weak", "network-wireless-signal-none": "wifi-off",
        "network-wireless-disconnected": "wifi-off", "network-wireless-offline": "wifi-off",
        "network-wireless-acquiring": "wifi-weak", "network-wireless-connected-100": "wifi",
        "network-wireless-connected-80": "wifi", "network-wireless-connected-60": "wifi-ok",
        "network-wireless-connected-40": "wifi-ok", "network-wireless-connected-20": "wifi-weak",
        "network-wireless-connected-00": "wifi-weak", "network-wired": "wired", "network-wired-activated": "wired",
        "network-offline": "wifi-off", "network-disconnect": "wifi-off", "network-connect": "wifi",
        "audio-volume-high": "volume", "audio-volume-medium": "volume-medium", "audio-volume-low": "volume-low",
        "audio-volume-muted": "volume-muted", "audio-volume-overamplified": "volume",
        "microphone-sensitivity-high": "mic", "microphone-sensitivity-medium": "mic",
        "microphone-sensitivity-low": "mic", "microphone-sensitivity-muted": "mic-muted",
        "audio-input-microphone": "mic", "audio-input-microphone-muted": "mic-muted",
        "bluetooth": "bluetooth", "bluetooth-active": "bluetooth", "bluetooth-disabled": "bluetooth",
        "bluetooth-paired": "bluetooth", "preferences-system-bluetooth": "bluetooth",
        "notifications": "bell", "notifications-disabled": "bell-off", "notification-inactive": "bell",
        "notification-active": "bell", "notification-disabled": "bell-off", "preferences-desktop-notification": "bell",
        "dialog-information": "info", "dialog-warning": "warning", "dialog-error": "error",
        "dialog-question": "info", "dialog-ok": "check", "security-high": "shield", "security-medium": "shield",
        "security-low": "shield", "dialog-password": "lock", "object-locked": "lock", "object-unlocked": "lock",
        "airplane-mode": "airplane", "weather-clear-night": "moon", "night-light": "moon",
        "media-playback-start": "play", "media-playback-pause": "pause", "media-playback-stop": "stop",
        "media-skip-forward": "next", "media-skip-backward": "previous", "view-visible": "eye",
        "preferences-system-notifications": "bell", "input-keyboard": "keyboard", "user-available": "user",
        "preferences-system-network": "wifi", "preferences-system-network-wireless": "wifi",
        "printer": "print", "printer-printing": "print", "download": "download",
    },
    "actions": {
        "edit-find": "search", "system-search": "search", "system-lock-screen": "lock", "system-shutdown": "power",
        "system-log-out": "power", "system-reboot": "refresh", "system-suspend": "moon", "go-previous": "left",
        "go-next": "right", "go-up": "up", "go-down": "down", "go-home": "home", "go-previous-symbolic": "left",
        "window-close": "close", "dialog-close": "close", "list-add": "add", "list-remove": "remove",
        "view-refresh": "refresh", "edit-copy": "copy", "edit-paste": "paste", "edit-cut": "close",
        "edit-delete": "trash", "edit-clear": "close", "edit-undo": "undo", "document-edit": "edit",
        "document-save": "save", "document-open": "folder", "document-new": "doc", "document-print": "print",
        "tab-new": "tab-new", "tab-close": "close", "application-menu": "menu", "open-menu": "menu",
        "view-list-icons": "grid", "view-list-details": "menu", "view-list-text": "menu",
        "view-split-left-right": "split", "view-split": "split", "view-fullscreen": "fullscreen",
        "zoom-in": "zoom-in", "zoom-out": "zoom-out", "view-preview": "eye", "overflow-menu": "menu",
        "application-exit": "power", "configure": "settings", "preferences-system": "settings",
        "show-menu": "menu", "folder-new": "add", "bookmark-new": "star", "edit-select-all": "check",
        "media-playback-start": "play", "media-playback-pause": "pause", "media-playback-stop": "stop",
        "media-skip-forward": "next", "media-skip-backward": "previous", "download": "download",
        "user-trash": "trash", "help-about": "info", "help-contents": "info", "dialog-ok-apply": "check",
        "dialog-cancel": "close", "dialog-ok": "check", "link": "link", "insert-link": "link",
        "network-connect": "wifi", "view-calendar": "calendar", "kt-add-filters": "menu", "show-sidebar": "split",
    },
    "devices": {
        "computer": "display", "video-display": "display", "audio-input-microphone": "mic",
        "input-keyboard": "keyboard", "printer": "print", "preferences-desktop-display": "display",
    },
    "categories": {"preferences-system": "settings", "applications-system": "settings"},
}

# Only the status context (tray, quick settings, OSDs) is ours; toolbar and app icons are handled
# elsewhere, so actions, devices and categories are left to the theme underneath.
NAMES = {"status": NAMES["status"]}

INDEX = """[Icon Theme]
Name=Tripppin Line
Comment=Thin white line glyphs for the system and toolbars; app icons come from Tripppin (Papirus)
Inherits=Tripppin,Papirus-Dark,breeze-dark,hicolor
Example=folder
Directories={dirs}

{sections}"""

CONTEXTS = {"status": "Status", "actions": "Actions", "devices": "Devices", "categories": "Categories"}


def main():
    names_written = 0
    for ctx, table in NAMES.items():
        d = OUT / "scalable" / ctx
        d.mkdir(parents=True, exist_ok=True)
        for name, glyph in table.items():
            for suffix in ("", "-symbolic"):
                n = name if name.endswith("-symbolic") else name + suffix
                (d / f"{n}.svg").write_text(svg(P[glyph]))
                names_written += 1
        if ctx == "status":
            # Batteries: the outline with bars for the level, a bolt while charging.
            for lvl in range(0, 101, 10):
                for charging in (False, True):
                    path = "M3 7h15v10H3zM21 10v4" + "".join(f"M{6 + 3 * i} 10v4" for i in range(3) if lvl > i * 33)
                    if charging:
                        path += "M11 8l-2 4h4l-2 4"
                    tag = "-charging" if charging else ""
                    for n in (f"battery-level-{lvl}{tag}-symbolic", f"battery-{lvl:03d}{tag}"):
                        (d / f"{n}.svg").write_text(svg(path))
            for n in ("battery", "battery-full", "battery-good", "battery-low", "battery-caution", "battery-empty",
                      "battery-missing", "battery-symbolic", "battery-full-symbolic", "battery-low-symbolic"):
                (d / f"{n}.svg").write_text(svg("M3 7h15v10H3zM21 10v4M6 10v4M9 10v4"))
    dirs = ",".join(f"scalable/{c}" for c in NAMES)
    sections = "\n".join(
        f"[scalable/{c}]\nSize=24\nMinSize=8\nMaxSize=512\nType=Scalable\nContext={CONTEXTS[c]}\n" for c in NAMES
    )
    (OUT / "index.theme").write_text(INDEX.format(dirs=dirs, sections=sections))


main()
