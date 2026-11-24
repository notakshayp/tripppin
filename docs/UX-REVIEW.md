# UX review and design system (2026-11-24)

From the owner's laptop test: all-lowercase copy hurt reading; text too small; corners and
padding pushed too far; the neon green glow and green borders felt loud while everything else
had no colour; Settings' window controls sat on top of the page; the computer was called
"fedora". This page is the rules we now follow and what was found, screen by screen.

Screens are checked with `scripts/screens.sh` (sway headless + grim in the OS image, no VM, no
GPU): `SCREENS="settings-wifi ask" CORNERS=round scripts/screens.sh out/screens`.

## Principles

| Area | Rule |
|---|---|
| Copy | Sentence case everywhere ("Power & battery", "Wi-Fi", "AI"). "Tripppin" always capitalised. Keys as "Super + Space". Commands and values stay as typed. |
| Hierarchy | One title per view: the window's title bar names the page; the page opens with a one-line status. Section headings are small semibold Inter, close to the group they name. |
| Type scale | In points, so Settings → Appearance → Text size scales it: caption/secondary 10 pt (≈13 px, the minimum), body 11 pt (≈15 px), lead 12 pt, title-3 13.5 pt, title-2 16.5 pt, title-1 19.5 pt. Inter for reading; DM Mono only for commands, values and shortcuts. System font is Inter 11. |
| Spacing | 4 px grid: `--s-1` 4 … `--s-7` 32. Rows 52 px high with 16 px side padding; pages 24 px side padding; 24 px between groups. |
| Radius | One scale, `--r-xs/sm/md/lg/xl` and `--r-control`, picked by Settings → Appearance → Corners: Sharp (2–6 px), Default (4–16 px, controls 8), Round (6–26 px, pill controls). Switches, avatars and the orb stay round. |
| Colour roles | Surfaces `--bg-sunken`, `--bg`, `--surface`, `--surface-2/3`. Hairlines `--line`, `--line-strong` (neutral, never green). Text `--fg`, `--fg-2`, `--fg-3`, `--fg-4`. Accent `--accent` (#57d99a) only for: primary button, selection tint (`--accent-soft`), focus ring (`--accent-line`), a switch that is on (`--forest`), live dots, links. |
| States | Hover: `--hover` tint. Selected: green tint plus a thin accent edge. Focus: one 2 px `--accent-line` outline for keyboard focus (GTK's blue replaced). Disabled: `--fg-4`. Danger: `--danger` text on `--danger-soft`. |
| Elevation | Neutral shadows only: `--shadow-sm/md/lg`. No glows anywhere. |
| Motion | One curve, cubic-bezier(0.32, 0.72, 0, 1), 160–360 ms; entrances move 8 px or scale from 96%; Reduce motion turns it off. |

## Findings

Severity: **H** hurts use, **M** looks unfinished or tiring, **L** polish. All H and M items
below are fixed in this change unless marked *open*.

| Screen | Sev | Finding | Fix |
|---|---|---|---|
| All | H | Everything lowercase, including names, dates, errors (`.to_lowercase()` on data). | Sentence case in every string; data shown as it is. |
| All | H | Body text 13 px, captions and status lines 11 px DM Mono. | Type scale above; Inter for all reading text. |
| All | M | Radii 14–28 px and pill buttons everywhere; inconsistent per widget. | One radius scale, Default noticeably tighter; Corners setting. |
| All | M | Green glows around the orb, ask bar, cards; green borders on hover and selection; the rest colourless. | Neutral shadows and hairlines; green as fill tint and small accents. |
| All | M | GTK's blue focus ring on entries (askpass, settings). | Accent focus ring, `accent_color` defined for our apps. |
| Settings (window) | H | No title bar: close/maximise floated over the page title; content could sit under them. | A real title bar row: "Settings" over the sidebar, the page name, window controls at the end; draggable. |
| Settings (window) | M | Empty 46 px band above the sidebar (an empty titlebar box). | Gone with the title bar. |
| Settings (window) | L | At 1240 px wide, cards stretch very wide. | *open*: a max content width needs a clamp widget. |
| Settings → every page | M | Page title 24 px heavy plus mono status; caption pulled up with a −12 px hack. | Title in the title bar; status in body text; captions scoped to pages. |
| Settings → sidebar | M | Icon tiles plus tinted rows doubled the selection signal. | Plain icons, green icon and soft tint on the selected row. |
| Settings → Wi-Fi, Bluetooth, Sound | L | Empty states as tall bordered rows. | Kept; rows are now the standard 52 px. |
| Settings → Accounts, Notifications | L | An empty list draws as a lone hairline. | *open*: hide empty cards. |
| Settings → Appearance | M | Thick 2 px green frame on the chosen wallpaper. | Thin accent edge and tint. Corners choice added. |
| Settings → AI | M | Mono model sizes, green full-width segments. | Inter, tinted segment; badges on tokens. |
| Settings → About | M | OS name and hardware lowercased; computer called "fedora". | Shown as reported; hostname fixed (below). |
| Orb | M | Glow halo and breathing glow. | Neutral shadow; working state breathes the border only. |
| Ask bar | M | 27 px pill with green halo; mono hint row; "history" link jammed against the hint. | Default-radius bar, Inter hints; *open*: hint row spacing lives in orb.rs. |
| Quick settings | M | 12 px labels, mono "clear all", tiles with green borders. | 13–16 px Inter, neutral tiles (forest fill when on, from the desktop branch), 384 px wide so the shortcut row fits. |
| Notifications | L | 12 px body, mono time. | 13–14 px Inter. |
| Bar | M | DM Mono 12 px, green hover on icons. | Inter 13 px with tabular digits, neutral hover. |
| Launcher (fuzzel) | L | 22 px radius, green-tinted border. | Default radius, neutral hairline, rounded selection. |
| Lock | M | Media title/artist forced lowercase. | As reported. |
| Greeter | M | Names lowercased; mono status. | Names as written; Inter status. |
| Welcome | H | Painted with the system theme: rendered light when no dark preference reached it. | Paints its own background; title bar styled. |
| First boot | M | GTK's default title bar, transparent window. | Our title bar and background; card flattened into the window. |
| Askpass | M | Mono help text, blue focus ring. | Inter, accent focus. |
| Switcher (Alt + Tab) | L | 2 px green frame on the selected app. | Tint and thin accent edge. |
| OSD | L | Mono name. | Inter name, mono value. |
| Calendar | L | Today filled bright green; mono date. | Soft tint for today, forest for the selection. |
| Power menu | M | 28 px radius, green hover border. | Default radius, subtle accent edge; red-tinted edge on shut down. |
| Shortcuts sheet | L | 24 px radius, mono everything. | Tokens; keys stay mono. |
| Windows (niri) | M | 16 px corners, 2 px neon focus ring. | 10 px (Corners setting), 1 px muted ring. |

## The corners option

Settings → Appearance → Corners (or `tripppin-ui corners sharp|default|round`) writes
`~/.config/tripppin/corners`. Our screens watch that file and restyle live; niri gets
`~/.config/niri/tripppin-corners.kdl`; the bar, quick settings and power menu get a small
`~/.config/<app>/style.css` that imports the system one (the bar and quick settings reload at
once); the launcher gets `~/.config/fuzzel/fuzzel.ini` including the system file. Default
removes the overrides; a file someone wrote themselves is never replaced (`crates/ui/src/look.rs`).
Other GTK apps keep the skel `gtk.css` radius (10 px) for now.

## Computer name

Fedora's os-release says `DEFAULT_HOSTNAME="fedora"`, which systemd uses when there is no
`/etc/hostname`. The image now says `tripppin`; ID, VERSION_ID, ID_LIKE, CPE_NAME and the support
fields stay Fedora's (bootc, bootc-image-builder, dnf and Anaconda read them). No VARIANT_ID.
The installer's kickstart presets `network --hostname=tripppin`. `tripppin-hostname.service`
runs once per machine and renames only "fedora" or an installer placeholder.
