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
- [ ] Electron and Firefox: dark mode through the portal (`color-scheme`), Inter as the sans font
- [x] Fonts: Inter (interface), DM Mono (code), fallbacks for Indic scripts and emoji
- [~] Icons: Papirus, folders recoloured to forest green
- [ ] Cursor: a Tripppin cursor theme (or Adwaita, recoloured)
- [ ] Sounds: a small Tripppin sound set (login, notification, error, screenshot)
- [ ] Dark and light: every component switches together

## Boot, login, lock

- [ ] Boot splash (Plymouth): the power-spark logo animating in, on ink
- [ ] Boot menu (GRUB): hidden unless Shift/Esc; themed when shown
- [x] Login screen (ours): clock, avatar, password pill, power menu
- [ ] Login: several people, switching user, caps-lock warning, "forgot password" help
- [~] Lock screen (ours): clock, what finished while you were away, password
- [ ] Lock: media controls, notifications count, battery, wake from sleep cleanly
- [ ] Idle: dim before locking, screen off, sleep (configurable in Settings)

## The shell

- [~] Bar (waybar): logo, workspaces, focused app, date, orb status, network, volume, battery
- [ ] Bar: Bluetooth, tray (for apps like Discord), keyboard layout, recording indicator, mic/camera in use
- [x] Quick settings (SwayNotificationCenter control centre): Wi-Fi, Bluetooth, do not disturb, night light,
      airplane mode, volume and brightness sliders, media player, power-mode, screenshot, lock
- [x] Notification centre (SwayNotificationCenter): history, grouping, actions, inline replies
- [x] Notifications: our cards top right
- [ ] Calendar pop-up from the clock, with upcoming events
- [x] Power menu (wlogout): lock, log out, sleep, restart, shut down
- [~] Volume, brightness, keyboard-light, caps-lock and mic-mute pop-ups (ours): volume, mute, mic and brightness done; keyboard light and caps lock to do
- [x] Launcher (fuzzel): apps
- [ ] Launcher: settings pages, files, calculator, "ask tripppin" handing over to the orb
- [ ] Window switcher (Alt+Tab) with previews; overview of all windows and workspaces
- [ ] Window controls: snap halves/quarters, maximise, minimise, always on top, move to workspace
- [ ] Keyboard shortcuts cheat sheet (Super+/), in our style
- [ ] Clipboard history (cliphist) with a picker
- [ ] Emoji and symbol picker
- [ ] Screenshots (grim + slurp + swappy): area, window, screen; annotate; copy or save
- [ ] Screen recording (wf-recorder) with a bar indicator
- [ ] Password prompts for admin actions (lxpolkit), styled
- [ ] On-screen keyboard (for touchscreens)
- [x] Wallpaper

## The orb (ours)

- [x] Resting in the corner; Super+Space opens the ask bar
- [~] Working card: live steps, status and clock, scrolling, stop
- [x] The full logo in the orb instead of the spark
- [ ] Voice input (hold Super+Space and talk)
- [ ] Ask about the screen or a selected file ("summarise this pdf")
- [ ] History of past requests; continue a conversation
- [ ] Permission prompts before risky steps (M4)

## Settings (ours, one window with a sidebar)

Simple sections are ours; deep ones open the themed specialist tool.

- [ ] Wi-Fi: nearby networks, connect with a password, forget, hidden network, hotspot, airplane mode
- [ ] Network details: IP, DNS, proxy, VPN (WireGuard, OpenVPN), Ethernet; advanced → nm-connection-editor
- [ ] Bluetooth: on/off, devices, pair, connect, battery levels; advanced → blueman
- [ ] Sound: output and input device, volume, mic level and test, per-app volume, alert sound; advanced → pavucontrol
- [ ] Display: resolution, scale, refresh rate, rotation, brightness, night light, arrangement → wdisplays
- [ ] Appearance: wallpaper (ours and your own), dark/light/auto, accent, text size, reduce motion
- [ ] Power & battery: health, time left, power mode, dim/off/sleep timing, lid action, low-battery behaviour
- [ ] AI: the model in use, switch or download, cloud key or own server, what it may do alone, history
- [ ] Apps: installed apps, update, remove, default apps (browser, mail, files, music, video)
- [ ] Notifications: do not disturb, schedule, per-app settings, lock-screen notifications
- [ ] Keyboard: layouts and input methods (ibus, Indian languages), repeat, shortcuts editor
- [ ] Mouse & touchpad: speed, tap to click, natural scroll, gestures
- [ ] Printers & scanners: add, default, queue → system-config-printer
- [ ] Accounts: name, picture, password, auto-login, add a person
- [ ] Date & time: time zone (automatic), 24-hour, date format
- [ ] Language & region
- [ ] Privacy: location, camera and microphone access, recent files, lock-screen content, firewall
- [ ] Accessibility: screen reader (orca), larger text, high contrast, zoom, sticky keys, reduce motion
- [ ] Storage: what's using space, clean up, external drives
- [ ] Updates: check, install (one click), automatic, roll back to the previous version
- [ ] About: device name, hardware, Tripppin version, licences, bill of materials

## Apps that come with it

- [x] Terminal (foot) opening into the AI shell
- [ ] Files (Thunar): sidebar, tabs, search, trash, drives, network shares, archive extract
- [ ] Browser: Firefox (Flatpak) on first boot, dark, Inter
- [ ] Text editor, image viewer, PDF viewer, calculator, archive manager, media player
- [ ] System monitor (gnome-system-monitor): processes, resources, end a stuck app
- [ ] Disks (gnome-disk-utility): format a USB stick, check health
- [ ] App store: browse and install from Flathub, in our style (part of Settings → Apps or a small app)
- [ ] Help / tips: what the orb can do, shortcuts, where things are

## System behaviour people notice

- [ ] USB drives: mount automatically, notification with "open" and "eject"
- [ ] Phone: file transfer (MTP), tethering
- [ ] Low battery warning, critical battery sleep
- [ ] Update available notification; restart into the new version; automatic rollback if it fails
- [ ] Crash: "app quit unexpectedly" with restart, not a silent close
- [ ] App not responding: "wait or force quit"
- [ ] Out of memory: the AI model unloads first
- [ ] First-boot welcome: model download (done), then a short tour

## Install and first run

- [~] Installer ISO (Fedora's, interactive); ours in M8
- [x] First boot: model download with progress
- [ ] First boot: a short tour, Wi-Fi if not connected, Firefox installed

## Website (trippp.in, tripppin-web repo)

- [ ] Rebuilt around the new designs: real screenshots of login, desktop, orb and Settings
- [ ] Download ISO button (Cloudflare R2, dl link from CI) with size, checksum and signature
- [ ] Setup page: write the USB (Mac, Windows, Linux), boot it, install, first boot
- [ ] No "scroll" hint on the hero
- [ ] Deployed to Firebase hosting
