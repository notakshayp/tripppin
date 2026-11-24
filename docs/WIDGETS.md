# Desktop widgets

Small cards on the desktop that show something live: system load, the local model, a
cricket score, a build status. Anyone can make one, and so can the AI: a widget is a folder
with a `widget.toml` and a script in any language that prints a little JSON. Tripppin draws
it in its own style, so every widget looks native and none can break the desktop.

This file is the reference for people and for the AI. It is installed at
`/usr/share/doc/tripppin/WIDGETS.md`, and `tripppin-ui widgets guide` prints it.

## Make a widget

1. Make a folder named after the widget (lowercase letters, digits, dashes), say `cricket/`.
2. Put a script in it that prints one JSON object to stdout (schema below) and exits.
3. Add `widget.toml` saying how to run it and where it goes.
4. Check it: `tripppin-ui widgets test ./cricket` runs it once and prints what it read, or
   the error.
5. Install it: `tripppin-ui widgets add ./cricket`. It appears within a couple of seconds.

The script runs with `sh -c "<command>"` in the widget's folder, again every `every`
seconds. Python 3, `curl`, `jq` and the usual shell tools are on every Tripppin system.

## widget.toml

```toml
name = "Cricket score"     # title when the script doesn't print one (optional)
command = "python3 score.py"
every = 60                 # seconds between runs (at least 2; default 60)
output = "HDMI-A-1"        # screen by connector name; leave out for the first screen
anchor = "top-right"       # top-left, top-right, bottom-left or bottom-right (default top-right)
x = 24                     # distance from that corner in pixels (default 24)
y = 24                     # at the top, measured from below the bar
width = 260                # optional, 120 to 1200
timeout = 20               # seconds before a run is stopped (default 20)
```

Unknown keys are an error (so a typo shows up in `widgets test`). Screen names come from
`tripppin-ui widgets outputs` (the same names as `niri msg outputs`). A widget pinned to a
screen that isn't plugged in waits for it instead of appearing elsewhere.

## What the script prints

One JSON object. Every field is optional.

```json
{
  "title": "India v Australia",
  "status": "ok",
  "items": [
    {"big": "245/3", "label": "India, 41.2 overs"},
    {"label": "Run rate", "value": "5.93"},
    {"label": "CPU", "value": "23%", "bar": 0.23},
    {"label": "Disk", "value": "91%", "bar": 0.91, "status": "warn"},
    {"text": "Kohli 87* (94)"},
    {"spark": [3, 5, 4, 8, 6], "label": "Runs per over"}
  ],
  "footer": "Updated 14:05"
}
```

- `title`: the card's heading. Sentence case: "Cricket score", not "CRICKET SCORE".
- `status`: `ok`, `warn` or `error`; colours the dot next to the title.
- `items`, top to bottom (up to 20):
  - `{"label": "…", "value": "…"}`: a line with a name on the left and a value on the right.
    Add `"bar": 0.42` (0 to 1, or a percentage like `42`) for a bar under it, and
    `"status": "warn"` or `"error"` to colour it.
  - `{"text": "…"}`: a line or short paragraph. A bare string `"…"` works too.
  - `{"big": "…", "label": "…"}`: a large number or score with a small caption.
  - `{"spark": [numbers], "label": "…"}`: a small line chart, oldest first (up to 120).
- `footer`: a small line at the bottom.

Plain text instead of JSON is fine too: each line becomes a text item. If the script exits
with an error, prints nothing, prints broken JSON or takes too long, the card shows the
reason in red; the desktop carries on. If a script prints several JSON lines, the last one
counts.

Keep scripts quick and quiet: fetch, print, exit. Use `every` instead of a loop. Don't
print secrets; read API keys from a file in the widget's folder. `$TRIPPPIN_WIDGET_DIR` is
the folder.

## Commands

| Command | What it does |
|---|---|
| `tripppin-ui widgets` | The host: draws the widgets. Started with the session (niri). |
| `tripppin-ui widgets add <folder>` | Copies a widget folder in, checks it once, shows it. Adding the same name again replaces it. |
| `tripppin-ui widgets add-system` | Turns on the built-in System widget. |
| `tripppin-ui widgets list` | Every widget: its screen, corner, refresh and how its last run went. |
| `tripppin-ui widgets test <name or folder>` | Runs it once and prints what it read, or the error. |
| `tripppin-ui widgets remove <name>` | Removes it. |
| `tripppin-ui widgets pin <name> <screen>` | Moves it to a screen (by connector name). |
| `tripppin-ui widgets outputs` | The screens' connector names. |
| `tripppin-ui widgets guide` | Prints this file. |

Widgets live in `~/.local/share/tripppin/widgets/<name>/`. Editing their files there works
too: the host notices and reloads them. Dragging a card moves it; where it was dropped and
which screen it was pinned to are kept in `~/.config/tripppin/widgets.json`, so the widget's
own files are never rewritten.

## Example: system

The built-in System widget (`tripppin-ui widgets add-system`) is an ordinary widget whose
script is part of Tripppin:

```toml
# ~/.local/share/tripppin/widgets/system/widget.toml
name = "System"
command = "tripppin-ui widgets system-json"
every = 3
anchor = "top-right"
```

It prints CPU (with a chart of the last 40 readings), memory, GPU load and video memory
where the driver says (AMD through sysfs, NVIDIA through `nvidia-smi`), the CPU
temperature, and the local AI model: off, loading, ready or working, with the memory
llama-server uses, and its speed in tokens per second when the server runs with `--metrics`.

The same thing as a few lines of shell, to show the shape:

```sh
#!/bin/sh
# load.sh: command = "sh load.sh", every = 5
read one five fifteen rest < /proc/loadavg
mem=$(awk '/^MemTotal/ {t=$2} /^MemAvailable/ {a=$2} END {printf "%.0f", (t - a) / t * 100}' /proc/meminfo)
cat <<JSON
{"title": "Load", "items": [
  {"label": "Last minute", "value": "$one"},
  {"label": "Memory", "value": "$mem%", "bar": $mem}
]}
JSON
```

## Example: cricket score

```toml
# cricket/widget.toml
name = "Cricket score"
command = "python3 score.py"
every = 60
anchor = "bottom-left"
```

```python
# cricket/score.py: prints the score of the match being followed as a Tripppin widget.
import json, urllib.request

# A JSON feed for the match: replace with a real one (a sports API, or a page to scrape).
URL = "https://example.com/cricket/live.json"

def show(title, items, status="ok", footer=None):
    print(json.dumps({"title": title, "status": status, "items": items, "footer": footer}))

try:
    with urllib.request.urlopen(URL, timeout=10) as r:
        m = json.load(r)
    team = m["batting"]
    show(
        f'{m["home"]} v {m["away"]}',
        [
            {"big": f'{m["runs"]}/{m["wickets"]}', "label": f'{team}, {m["overs"]} overs'},
            {"label": "Run rate", "value": f'{m["runs"] / max(float(m["overs"]), 0.1):.2f}'},
            {"text": m.get("summary", "")},
        ],
        footer=m.get("updated"),
    )
except Exception as e:
    # Still a widget: say what went wrong instead of crashing.
    show("Cricket score", [{"text": f"Couldn't get the score: {e}"}], status="error")
```

To scrape a page instead, fetch the HTML the same way and pull the numbers out with `re`;
Python's standard library is enough, so nothing needs installing.

## For the AI

When someone asks for a widget ("show me the cricket score", "a widget with my GPU
temperature"):

1. Write the folder in a scratch directory: `widget.toml` and one script. Prefer Python 3
   (standard library only) or shell; print the JSON above and exit.
2. Run `tripppin-ui widgets test <folder>` and fix what it reports.
3. Run `tripppin-ui widgets add <folder>`. If they named a screen, run
   `tripppin-ui widgets outputs` to find its connector name and put it in `output`, or
   `tripppin-ui widgets pin <name> <screen>`.
4. Tell them it's on the desktop, and that `tripppin-ui widgets remove <name>` takes it away.

## Why our own host (research, November 2026)

What exists, and why we don't use it for this:

| Option | Licence | What a widget is | For us |
|---|---|---|---|
| [eww](https://github.com/elkowar/eww) | MIT | Its own Lisp-like language (yuck) plus SCSS, GTK 3, one config for every widget | A niche language small models get wrong; GTK 3 next to our GTK 4; one shared config, so one bad widget breaks them all |
| [AGS / Astal](https://github.com/Aylur/ags) | GPL-3.0 (AGS), LGPL-2.1 (Astal) | TypeScript and JSX on gjs, bundled with esbuild | A JavaScript runtime and toolchain on every machine; widgets are whole programs that can draw anything, so generated ones look off-brand and can hang the shell |
| [Quickshell](https://quickshell.org) | LGPL-3.0 | QML on Qt 6 | Brings all of Qt 6 to a GTK desktop; QML plus its own API is a lot for a small model |
| waybar custom modules | MIT | A script printing JSON (`text`, `tooltip`, `class`, `percentage`) | The right idea, but only in the bar, and the bar config is ours |
| [Conky](https://github.com/brndnmtthws/conky) | GPL-3.0 | A text template with Lua | X11 first, partial Wayland; its own template language; dated look |
| KDE Plasma widgets, GNOME extensions | various | Tied to their shells | Not our shell |
| Übersicht (macOS) | GPL-3.0 | A command plus a JSX renderer | The model to copy: a command makes the data; we keep the rendering |

All of these could only be called as separate programs anyway (we don't link GPL code),
and each adds a runtime and a design language of its own. The part a model must get right
should be as small as possible: a script that prints a JSON object with four kinds of item
is something even a small local model writes correctly first time, in any language it
knows, and `widgets test` tells it exactly what's wrong when it doesn't. The rendering
stays ours (GTK 4, layer-shell, our stylesheet), so every widget is calm and consistent,
and a broken widget can only break its own card.

Weight: the host is one more `tripppin-ui` process; it wakes once a second to look for
changed folders and runs each script on its own schedule, in a thread with a timeout.

## Not yet

- Clicking a widget (open a link or run a command).
- Turning the System widget on from Settings (for now `tripppin-ui widgets add-system`).
- Light mode for the cards (dark only, like the rest of the desktop).
