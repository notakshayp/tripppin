# The Tripppin desktop: everything a person touches

The rule: **nothing a person can open looks or behaves like a different OS.** Every screen,
dialog, menu and app, ours or installed later, follows the Tripppin style: ink, paper and
forest green; Inter and DM Mono; lowercase, sparse copy; rounded cards.

How we get there:

1. **One theme for every toolkit**, so apps people install match too: GTK 3, GTK 4 and
   libadwaita, Qt 5 and 6, Flatpak apps, Electron and Firefox (they follow GTK's dark mode),
   plus icons, cursor, fonts and sounds.
2. **Lightweight open-source components, restyled to the fullest** with their own CSS or
   config where they have it. They run as separate programs, so GPL ones are fine
   (see the Rules in AGENTS.md).
3. **Our own (`tripppin-ui`, Rust + GTK 4)** only where nothing good exists or the screen is
   what makes Tripppin itself: the orb, login, lock, first boot, installer, pop-ups, Settings.

Status: `[x]` done and checked in the VM against the mockup · `[~]` works, styling or
details still open · `[ ]` not started. Everything is verified by screenshot in the VM.

## Theme for every app

- [~] GTK 4 / libadwaita: accent, colours, fonts, radii via `/etc/skel/.config/gtk-4.0/gtk.css` and dconf
- [~] GTK 3: adw-gtk3 as the base, recoloured with our palette (`gtk-3.0/gtk.css`)
- [~] Qt 5 / 6: qt6ct + qt5ct with a Tripppin colour scheme, Fusion style, Inter
- [~] Flatpak apps: system override exposing the GTK config; the GTK 3 theme as a Flatpak extension
- [x] Fonts: Inter (interface), DM Mono (code), fallbacks for Indic scripts and emoji
- [~] Icons: Papirus, folders recoloured to forest green

## Boot, login, lock

- [~] Boot splash (Plymouth): the power-spark logo animating in (the ring draws, the × spins in, then turns gently), on ink (our script theme in the initramfs, quiet boot; it runs in the VM (journal); seeing it is checked on the laptop)
- [~] Boot menu (GRUB): hidden unless Shift/Esc; themed when shown (hidden with a one-second Esc window through bootupd, on new installs; a themed menu to do)
- [x] Login screen (ours): clock, avatar, password pill, power menu
- [x] Login: several people, switching user, caps-lock warning, "forgot password" help (a chip per person plus "other"; caps-lock warning from the keyboard light; honest help text; checked in the VM with two people)
- [~] Lock screen (ours): clock, what finished while you were away, password
- [~] Lock: media controls, notifications count, battery, wake from sleep cleanly (media card with back/play/next, notification count, battery and caps-lock warning built and checked in a preview in the VM; waking from sleep checked on the laptop)
- [x] Idle: dim before locking, screen off, sleep (configurable in Settings) (the screen fades 30 s before the lock; checked in the VM through swayidle; real input undims, which the VM's virtual keyboard can't show)

## The shell

- [~] Bar (waybar): logo, workspaces, focused app, date, orb status, network, volume, battery
- [x] Bar: Bluetooth, tray (for apps like Discord), keyboard layout, recording indicator, mic/camera in use (mic and screen sharing via waybar's privacy module; camera use has no reliable signal under PipeWire yet)
- [x] Quick settings (SwayNotificationCenter control centre): Wi-Fi, Bluetooth, do not disturb, night light,
      airplane mode, volume and brightness sliders, media player, power-mode, screenshot, lock
- [x] Notification centre (SwayNotificationCenter): history, grouping, actions, inline replies
- [x] Notifications: our cards top right
- [~] Calendar pop-up from the clock, with upcoming events (the month and today, opened and closed from the clock; events wait for calendar accounts)
- [x] Power menu (wlogout): lock, log out, sleep, restart, shut down
- [x] Volume, brightness, keyboard-light, caps-lock and mic-mute pop-ups (ours) (caps lock watched by the orb from the keyboard light, checked in the VM; the keyboard light keys checked on the laptop)
- [x] Launcher (fuzzel): apps
- [~] Launcher: settings pages, files, calculator, "ask tripppin" handing over to the orb (settings and every page, with search words, and "ask tripppin" are in the launcher; sums go to the orb; searching files to do)
- [x] Window switcher (Alt+Tab) with previews; overview of all windows and workspaces (alt + tab: our switcher, most recent first, app icons; super + tab: niri's overview with live previews; checked in the VM)
- [~] Window controls: snap halves/quarters, maximise, minimise, always on top, move to workspace (halves, quarters, fill, centre and move to workspace done; niri has no minimise or always-on-top; Settings is too wide for half of a small screen)
- [x] Keyboard shortcuts cheat sheet (Super+/), in our style
- [x] Clipboard history (cliphist) with a picker (Super + V; checked in the VM: text and images, picking puts it back)
- [~] Emoji and symbol picker (Super + .; Unicode's names, typed with wtype and copied; search checked in the VM; the colour emoji font comes with the next image)
- [x] Screenshots (grim + slurp + swappy): area, window, screen; annotate; copy or save (niri's own capture: print, ctrl/alt + print, saved and copied; super + shift + s draws on the last one in swappy; checked in the VM)
- [x] Screen recording (wf-recorder) with a bar indicator (super + shift + r, alt for an area; red "● rec" in the bar stops it; checked in the VM)
- [~] Password prompts for admin actions (lxpolkit), styled (mate-polkit, GTK3: appears in our theme with the green action button, checked in the VM; entering the password checked with a real keyboard on the laptop; its own sentences stay capitalised)
- [x] Wallpaper

## The orb (ours)

- [x] Resting in the corner; Super+Space opens the ask bar
- [~] Working card: live steps, status and clock, scrolling, stop
- [x] The full logo in the orb instead of the spark
- [x] The "super + space" hint shows only until the orb has been used once (it draws over windows near the corner)

## Settings (ours, one window with a sidebar)

Simple sections are ours; deep ones open the themed specialist tool.

- [~] Wi-Fi: nearby networks, connect with a password, forget, hidden network, hotspot, airplane mode (built; the VM has no Wi-Fi card, so connecting is checked on real hardware)
- [x] Network details: IP, DNS, proxy, VPN (WireGuard, OpenVPN), Ethernet; advanced → nm-connection-editor (checked in the VM: cable details, a WireGuard file imported and connected; OpenVPN plugin in the image)
- [~] Bluetooth: on/off, devices, pair, connect, battery levels; advanced → blueman (built; the VM has no Bluetooth, so pairing is checked on real hardware)
- [~] Sound: output and input device, volume, mic level and test, per-app volume, alert sound; advanced → pavucontrol (devices, volume, mute, per-app built; mic test and alert sound to do; real devices checked on hardware)
- [~] Display: resolution, scale, refresh rate, rotation, brightness, night light, arrangement → wdisplays (built; mode changes and brightness checked on hardware)
- [~] Appearance: wallpaper (ours and your own), dark/light/auto, accent, text size, reduce motion (wallpapers, your own, text size, reduce motion done; light mode and accent to do)
- [~] Power & battery: health, time left, power mode, dim/off/sleep timing, lid action, low-battery behaviour (battery, mode, lock/screen-off/sleep timing built; lid action and low-battery to do; battery and mode checked on hardware)
- [~] AI: the model in use, switch or download, cloud key or own server, what it may do alone, history (model in use, switch, download, service restart, other server with a key built; permission prompts come with M4, history to do)
- [x] Apps: installed apps, update, remove, default apps (browser, mail, files, music, video)
- [~] Notifications: do not disturb, schedule, per-app settings, lock-screen notifications (do not disturb, quiet hours, muting apps built; lock-screen notifications come with the lock item)
- [~] Keyboard: layouts and input methods (ibus, Indian languages), repeat, shortcuts editor (layouts, switch keys, repeat and a shortcuts list built; ibus transliteration and editing shortcuts to do)
- [x] Mouse & touchpad: speed, tap to click, natural scroll, gestures (touchpad settings show only when one is found; checked on hardware)
- [x] Printers & scanners: add, default, queue → system-config-printer (checked in the VM with a test queue: state, default, queued job, cancel; real printers and scanners checked on hardware)
- [~] Accounts: name, picture, password, auto-login, add a person (all built; name checked in the VM; password, auto-login and adding a person run through sudo with your password and are checked on the laptop)
- [~] Date & time: time zone (automatic), 24-hour, date format (zone, automatic time, 12/24-hour built and checked; picking the zone from location to do; date format follows region formats)
- [~] Language & region (language and region formats built and checked with the new image: Indian languages and their fonts are installed; our own screens are English only)
- [~] Privacy: location, camera and microphone access, recent files, lock-screen content, firewall (built; firewall checked on in the new image; per-app microphone control is not possible with PipeWire's sound access today)
- [~] Accessibility: screen reader (orca), larger text, high contrast, zoom, sticky keys, reduce motion (screen reader in the image, its voice checked on the laptop; larger text, high contrast, pointer size, reduce motion built; niri has no zoom or sticky keys yet)
- [~] Storage: what's using space, clean up, external drives (usage and clean-up checked in the VM; ejecting a USB drive checked on hardware)
- [x] Updates: check, install (one click), automatic, roll back to the previous version (daily download, never a surprise restart: the version starts at the next restart, which the page offers; checked in the VM)
- [~] About: device name, hardware, Tripppin version, licences, bill of materials (name, hardware, storage, version, licence built; a browsable bill of materials waits for CI to publish an SBOM)

## Apps that come with it

- [x] Terminal (foot) opening into the AI shell
- [x] Files (Thunar): sidebar, tabs, search, trash, drives, network shares, archive extract (with volume manager, archive plugin, thumbnails, gvfs; green folders; checked in the VM)
- [x] Browser: Firefox (Flatpak) on first boot, dark, Inter (installed by a one-shot service once online; dark and Inter checked in the VM)
- [x] Text editor, image viewer, PDF viewer, calculator, archive manager, media player (Text Editor with our colour scheme, Loupe, Papers, Calculator, File Roller, Celluloid; set as defaults)
- [x] System monitor (gnome-system-monitor): processes, resources, end a stuck app
- [x] Disks (gnome-disk-utility): format a USB stick, check health
- [x] App store: browse and install from Flathub, in our style (part of Settings → Apps or a small app) (search and install in Settings → Apps; phrases search by the longest word and rank by the rest)
- [x] Help / tips: what the orb can do, shortcuts, where things are ("welcome & tips" in the launcher)

## System behaviour people notice

- [~] USB drives: mount automatically, notification with "open" and "eject" (udiskie starts with the desktop; a real USB stick checked on the laptop)
- [~] Phone: file transfer (MTP), tethering (gvfs-mtp and NetworkManager in the image; a phone checked on the laptop)
- [~] Low battery warning, critical battery sleep (warnings at 10% and 5% from the orb; UPower's critical action; checked on the laptop)
- [x] Update available notification; restart into the new version; automatic rollback if it fails (the orb notifies when an update is staged; greenboot health check and rollback trigger on)
- [x] Crash: "app quit unexpectedly" with restart, not a silent close (from systemd-coredump, with "open it again")
- [x] Out of memory: the AI model unloads first (OOMScoreAdjust=800 on the model service)
- [x] First-boot welcome: model download (done), then a short tour

## Install and first run

- [~] Installer ISO (Fedora's, interactive); ours in M8
- [x] First boot: model download with progress
- [x] First boot: a short tour, Wi-Fi if not connected, Firefox installed (the tour opens once at the first login, with a wi-fi card first when offline; Firefox installs once online)

## Polish pass (after the first ISO)

From the owner's first hands-on test: sluggish, soft text, too large, no icons, no motion.

- [x] Sharp text: greyscale antialiasing with slight hinting (fontconfig, GTK 3 and 4, dconf), and a whole-number display scale at login (`tripppin-ui displays`: 2 on dense screens, else 1; niri's own guess was 1.25, which blurred text and enlarged everything). Settings → Display keeps a person's own choice
- [x] Compact: Inter 10 everywhere, and every screen of ours, the bar, quick settings and the launcher resized for scale 1
- [x] Speed: the GPU draws our screens (the software renderer only without 3D); Settings builds each page when first opened instead of all 21 at start
- [x] Icons: Lucide as a font (bar, quick settings tiles and sliders, Settings sidebar, pop-ups, the orb's card); our own notifications carry icons
- [x] Orb: smaller (44 px); drag it and it springs to the nearest corner; right-click tucks it into a dot; the working card minimises into it and opens from its corner
- [x] Motion: niri springs for every window movement, open and close shaders (grow from 92% with a slight settle, shrink away), one easing curve in every stylesheet, press-in on buttons, entrances for every pop-up and page, exits that fade toward where things came from, a frame-clock spinner, gliding pop-up bars. Reduce motion turns it all off
- [ ] Checked on the laptop: sharpness and scale on a real panel, animation smoothness on a real GPU

## Second round (owner's notes, 2026-11-24)

- [x] Windows stay usable: a floating window is never left under the bar, bigger than the screen or dragged off it (`keeper.rs` watches niri's event stream and nudges it back once it settles). Super + drag moves, Super + right-drag resizes, Super + C or ↓ centres; all in the shortcuts sheet
- [x] Apps: Super + A (niri can't bind a tap of Super alone), the logo in the bar, or type an app's name after Super + Space and pick it (an exact name and Enter opens it)
- [x] Quick settings: Wi-Fi and Bluetooth tiles say so when the computer has no adapter instead of looking on; a row of shortcuts to the Settings pages (networks, devices, sound, display)
- [x] Orb: every step opens on a click (the whole command and the end of its output); follow-up questions in the card; earlier requests (the orb's and the terminal's) listed, readable step by step and continued; "continue in terminal" opens that conversation in the AI shell with every step
- [x] AI: a task notebook the model sees at every step (its plan and notes, one line per step with the result), and compaction that drops old steps instead of failing when the context fills; sessions saved as JSON and Markdown in ~/.local/state/tripppin/sessions
- [x] AI administrator commands: a password pop-up that shows the command (sudo askpass), or Settings → AI lets installs, services or updates run without a password
- [x] Settings → AI: guided cloud providers (OpenAI, Anthropic, Gemini, DeepSeek, Sarvam AI, OpenRouter, Groq, your own server) with a key link and the provider's model list; other local models (Gemma 4 E4B, 12B, 26B-A4B, Granite 4.1 8B)
- [x] Checked in the VM: oversized windows shrink back on screen, maximise and restore, app matches in the ask bar, the new card with a two-step request, "continue in terminal" and `/history`, the Wi-Fi tile with no adapter, the Settings shortcuts row, the volume slider (now reads the real level), Settings → AI (local and cloud views), the sudo pop-up end to end (`sudo id -u` from the AI with no terminal), passwordless groups on and off
- [x] Snapping like Rectangle: drag a window past the left or right edge for a half, the top for fill, a corner for a quarter, with a translucent preview while it's held there (keeper.rs; niri's IPC has no pointer or drag signal, so a drag is a run of moves without a resize); dragging a snapped window away gives back its old size. Ctrl + Alt + arrows halves (again for 1/3, then 2/3), U I J K quarters, Return fill, C centre, Backspace back to before snapping (remembered per window). Checked in nested niri 26.04 (headless sway in a container) with simulated drags
- [~] Desktop widgets (docs/WIDGETS.md): `tripppin-ui widgets` draws a card per folder in ~/.local/share/tripppin/widgets from a script's JSON, in our style; add, list, test, remove, pin to a screen; a System widget (CPU, memory, GPU, temperature, the AI model) with `widgets add-system`. Checked in nested niri; dragging a card needs a real pointer
- [ ] Needs a pointer (not testable over ssh): opening a step, sending a follow-up, the history list, dragging a window under the bar

## The mark and the way in (owner's notes, 2026-11-24)

- [x] The spark is an ×: turned 45° so its long points are two arrows meeting in the middle, a little larger (points 42, centred at 100, 46) with the ring's opening widened from ±44° to ±52° so it never crowds the ring's ends. One geometry for everything, in `crates/trip/src/logo.rs` (the UI draws through `common::draw_mark_posed`)
- [x] No sparkle burst anywhere: the intro is the ring drawing itself and the × spinning in half a turn and settling; while something loads, the × makes a gentle quarter turn every 2.4 s (terminal, `brand/logo-animated.svg`, Plymouth, the splash)
- [x] Every raster regenerated by `scripts/brand-assets.sh` (cargo + rsvg-convert in a Fedora container): social images, app icon, bar logo, the four wallpapers, Plymouth's ring frames and spark, `brand/terminal-logo.png`
- [x] No text console between the login screen and the desktop. Cause: greetd starts the session only after the greeter's niri has exited, so for about a second tty1 was back in text mode, showing the greeter niri's log lines (niri logs to stderr, which greetd points at the VT). Now the greeter (`/usr/libexec/tripppin-greeter-session`) and the session (`/usr/libexec/tripppin-session`) send niri's output to the journal and paint the console's whole palette ink (`tripppin-ink-console`), the login screen fades to ink before quitting, and niri's own background is ink
- [x] A splash on the way in: `tripppin-ui splash` (first in the niri session) covers the screens in ink with the mark's intro and fades once niri lists the bar's and the orb's surfaces (at least 1.5 s, at most 6 s)
- [x] The terminal logo stays for real terminals (text consoles, ssh); `tripppin-logo.sh` never runs it on tty1 or inside a graphical session
- [ ] Checked in the VM and on the laptop: the login-to-desktop handover, the splash fading out, the Plymouth animation (Plymouth's script can't be run outside a boot)
- [ ] Later: hand Plymouth over to the login screen without a text frame (`plymouth deactivate` / `quit --retain-splash`, as GDM does), so boot is ink from the splash to the login screen too

## Later (after this desktop lands)

Not blockers for the desktop: bigger features, things niri can't do yet, and checks that need
real hardware.

- [ ] Electron apps: dark mode through the portal and Inter (Firefox is checked; an Electron app like Discord is checked on the laptop)
- [ ] Cursor: a Tripppin cursor theme (Adwaita for now)
- [ ] Sounds: a small Tripppin sound set (login, notification, error, screenshot)
- [ ] Light mode: every component switching together (dark only for now; Appearance says so)
- [ ] On-screen keyboard for touchscreens
- [ ] Voice input: hold Super + Space and talk (with a local speech model)
- [ ] Ask about the screen or a selected file ("summarise this pdf")
- [ ] History of past requests in the orb; continue a conversation
- [ ] Permission prompts before risky steps (M4, with the safety work)
- [ ] App not responding: "wait or force quit" (niri has no hang detection yet; System Monitor can end a stuck app)

## Website (trippp.in, tripppin-web repo)

- [x] Rebuilt around the new designs: the ask demo is the real desktop (bar, Files, the orb's ask bar and working card) rebuilt in HTML from our styles, playing real requests; no screenshot gallery
- [x] Download ISO button (Cloudflare R2, dl link from CI) with size, checksum and signature
- [x] Setup page: write the USB (Mac, Windows, Linux), boot it, install, first boot
- [x] No "scroll" hint on the hero
- [x] Deployed to Firebase hosting
