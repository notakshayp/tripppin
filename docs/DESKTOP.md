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

- [~] Boot splash (Plymouth): the power-spark logo animating in, on ink (our script theme in the initramfs, quiet boot; it runs in the VM (journal); seeing it is checked on the laptop)
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
