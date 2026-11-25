# Tripppin build plan

The full picture, with diagrams, is in `site/roadmap.html`. This file is the working checklist: what's done, what's next and what's blocked. Update it in the same PR as the work.

## Decisions in force

| Area | Decision |
|---|---|
| Base | Fedora bootc 45, image-based. The whole OS is one signed OCI image; machines update atomically and roll back on failed boots. |
| Language | Rust for everything we write (edition 2024, MSRV 1.85). |
| Model engine | llama.cpp server as a supervised child process; Vulkan by default, NVIDIA image variant later. |
| Desktop | KDE Plasma 6 on KWin (Wayland), restyled end to end ("Tripppin" look-and-feel, colour scheme, Plasma style, Konsole profile), with our own screens in Rust (orb, login, Settings hub). Replaced niri on 2026-11-25: niri lacked desktop icons, right-click menus and a complete file manager. Hyprland was the first plan; COSMIC was dropped as too generic. |
| Shell | Plain text goes to the model. `!cmd` runs a raw command; `/term` opens a classic shell. |
| Safety | Code is the boundary (taint labels, policy engine, sandbox); an AI verifier checks intent on top. YOLO keeps two exceptions. |
| Minimum | 16 GB memory for local AI; below that, a cloud key or the user's own server. |
| Registry | `ghcr.io/notakshayp/tripppin`, tags `nightly`, `<sha>`; later `testing`, `beta`, `stable`. |
| License | Apache-2.0 for our code. |
| Website | Separate repo: `notakshayp/tripppin-web` (trippp.in). |

## Milestones

Reordered on 2026-11-20: the AI comes before the desktop. The AI shell runs in a plain terminal, so it can be proven on the console build before any desktop work.

| # | Milestone | You can try | Status |
|---|---|---|---|
| M0 | Foundations | CI on every PR | mostly done |
| M1 | Console build (ISO 1) | Boot, log in, see the animated logo | **in progress** |
| M2 | Model service | `trip ask "hello"` answers locally | **next** |
| M3 | AI shell + harness (ISO 2) | Sentences in the terminal do things | |
| M4 | Safety | Booby-trapped pages get refused | |
| M5 | Desktop + orb (ISO 3) | Branded desktop with the orb | **shipped**: first ISO on R2, trippp.in live; laptop check left |
| M6 | Update loop + channels | Overnight updates, bad build rolls back | |
| M7 | Agents + bring your own model | Background jobs, cloud keys, own server | |
| M8 | Installer | Five-minute install, our own UI | |
| M9 | Public beta | Download from trippp.in | |

## M0: Foundations

- [x] Cargo workspace (`crates/*`), Apache-2.0 license, rustfmt config
- [x] `hwdetect` crate: memory, CPU flags and GPUs from `/proc` and `/sys`, plus `nvidia-smi`; unit tests on captured fixtures
- [x] `models/catalog.toml` and tier selection (16 GB → 4B, 8 GB VRAM → 9B, 32 GB → 35B-A3B, 24 GB VRAM → 27B)
- [x] `trip hw` command (text and `--json`)
- [x] `os/Containerfile`: Fedora bootc 45 pinned by digest, our binary inside, `bootc container lint`
- [x] `ci.yml`: fmt, clippy `-D warnings`, tests on x86_64 and arm64, runs `trip hw` on the runner
- [x] `image.yml`: build the image on PRs; on main and nightly, push to GHCR as `nightly` and `<sha>`
- [x] Dependabot for cargo, actions and the base image digests
- [x] First green CI run and first image in GHCR (`ghcr.io/notakshayp/tripppin:nightly`)
- [ ] Branch protection on `main`: not available for private repos on the free plan. Until then every change goes through a PR with green checks
- [x] Image signing: cosign key pair; private key and password in repo secrets; `cosign.pub` in the repo; every pushed image signed and verified
- [x] Vulnerability gates: cargo-deny on every PR and weekly; image SBOM (SPDX) and grype scan, failing on critical issues that have a fix
- [ ] Lima dev VM recipe for the Mac (`dev/lima.yaml`), with podman and `bcvk`
- [ ] Pin GitHub Actions by commit hash

- [ ] Re-add toolbox once Fedora ships a build without CVE-2026-78663 (removed from the image for now)

## M1: Console build

- [ ] greenboot health checks: network, our services
- [x] Multi-arch image: amd64 and arm64 built natively, joined under one tag, list and per-arch images signed
- [x] `disk.yml`: bootc-image-builder → x86_64 ISO + qcow2, aarch64 qcow2 (for UTM on the Mac); compressed, split under 2 GiB, checksums signed, attached to the `nightly` pre-release
- [x] Test login secrets (`TEST_USER_PASSWORD`, `TEST_USER_SSH_KEY`)
- [x] Console branding: os-release name, login banner with the logo, `trip logo` animation on login
- [ ] First disk build attached to the `nightly` release
- [ ] `boot-test.yml`: boot the qcow2 under KVM, wait for `multi-user.target`, check `trip hw` and `bootc status`

## M2: Model service, next

- [x] llama.cpp v1.0.0 built in the image (Vulkan backend, CPU fallback), Mesa Vulkan drivers; CI checks the binary and the flags we pass
- [x] Model catalog: Qwen3.5 Q4_K_M files (unsloth builds, Apache-2.0) pinned to repository revisions with SHA-256 and context sizes
- [ ] Sign the catalog; later, quantize the official weights ourselves
- [x] `trip model pull` / `list`: download with resume and SHA-256 check into `/var/lib/tripppin/models`; TLS trusts the OS certificate store
- [x] `tripppin-llm.service`: `trip model serve` execs llama-server on 127.0.0.1:8086; DynamicUser, read-only models, localhost-only IP filter, idle unload after 15 min
- [x] `trip ask "…"`: streams the answer and reports time to first word and tokens/s
- [x] Memory-safe serving sized per machine ([HARNESS.md](HARNESS.md)): model shape read from the GGUF header; context and placement (CPU, integrated GPU within its GTT share, graphics card via `--fit`) chosen so model + cache fit next to the desktop; q8_0 KV cache with flash attention, context checkpoints, a bounded prompt cache, cache reuse
- [ ] Measure speed (tokens per second, time to first word) per hardware tier; publish in the plan
- [ ] Eval runner skeleton with a first handful of recorded requests

## M3: AI shell + harness

- [ ] Harness daemon: router, per-category tool sets, the model's native tool-call grammar, validation, step limits
- [ ] First tools: system info, files (read), web search (local SearXNG), web fetch
- [x] The AI shell (`trip shell`), default for interactive terminals: plain text goes to the model as a single question, `/chat` keeps a conversation, `/terminal` opens a normal shell, `!cmd` runs one command, `/setup` downloads the model; streaming and Ctrl-C
- [x] Shell conversation view (`trip::tui`): "You" / "Tripppin" gutter, answers rendered from Markdown (lists, aligned tables, framed code), steps as one row each (spinner → ✓/✗, time, newest output while running), a live status line that always says what it's waiting on, the plan as a checklist, a token line after each answer (in, cached, out, tok/s, context fill, session total); welcome header, mode prompt, footer with model and context, Tab completion for /commands; `trip shell --demo` replays a canned session for screenshots
- [x] Agent loop in `trip`: the model calls tools until it answers in plain text (25-step cap, Ctrl-C stops it)
- [x] Tools: `exec` (bash, live output, timeout, sudo prompts work), `read_file`, `write_file`
- [x] Task notebook and compaction for small models: the model's plan and notes plus one line per step, shown at every step; old steps dropped when the context fills (the notebook keeps them); every session saved and listed (`/history`, the orb's history), continued in the terminal
- [x] Follow mode: while the orb runs a request it appends each agent event as a JSON line to `<session>.live`; "Continue in terminal" renders the history, then follows the run live with the shell's own rendering (Ctrl+C writes `<session>.stop`, which the orb honours), then carries on as a chat
- [x] Task plan (`plan` / `done` tools): short steps, one at a time, each checked; kept in the session across compaction and `--continue`; stuck detection (repeats, no progress, failures in a row) nudges, then makes the model stop and explain
- [x] Compaction on the server's real token counts with headroom (85 % → 55 %); the notebook sent as its own message so the prompt cache holds; what was dropped recorded in the session; UI events for waiting, token usage and the plan ([HARNESS.md](HARNESS.md))
- [x] Administrator commands: sudo asks through a pop-up showing the command; Settings → AI can allow installs, services or updates without a password
- [x] AI permissions ([AI-PERMISSIONS.md](AI-PERMISSIONS.md)): a level (ask every time, allow what I pick, full administrator with risky commands still asking) and nine areas, kept in a root-owned policy and a sudoers rule checked by visudo; the agent's sudo never reuses a cached password, and every administrator command is logged and listed under Settings → AI → Activity
- [ ] Tell the agent its permissions: include `trip::permissions::summary()` in the harness system prompt
- [x] Tool-call verifier: unknown tools, bad JSON, missing or mistyped fields and calls written as text go back to the model with the exact problem; 3 retries before giving up
- [x] Flatpak with Flathub in the image, so the agent can install apps
- [ ] Safety: permission modes, a second check on risky steps, undo (M4)
- [ ] Record/replay tests; eval gate on PRs
- [ ] ISO 2

## M7: Agents + bring your own model (started early)

- [x] Settings → AI: guided cloud providers (OpenAI, Anthropic, Google Gemini, DeepSeek, Sarvam AI, OpenRouter, Groq) or your own server, with the provider's model list
- [x] Models appear only after the key checks out, from each provider's own list endpoint (Anthropic's headers, OpenRouter's key check and tool-capable models, Groq's active ones, Sarvam's fixed pair), with a default preselected, search, and plain errors for a refused key or no network
- [x] Several local models: download any that fits (size, memory need, progress, cancel and resume), keep several, switch the one in use (restarts `tripppin-llm`), delete to free space (`trip::models`)
- [x] Other local models with native tool calling: Gemma 4 (E4B, 12B, 26B-A4B), Granite 4.1 8B; each family gets its own sampling
- [ ] Background jobs that outlive the request

## M5: Desktop + orb

The full checklist, with what's verified where, is [DESKTOP.md](DESKTOP.md).

- [x] Desktop session: niri (floating-first, our focus ring), greetd with our login screen, PipeWire, NetworkManager, Inter and DM Mono, Flatpak with Flathub
- [x] Plymouth boot splash with the power spark, wallpapers, a hidden boot menu
- [x] The orb overlay: ask, live steps, the result; the logo in the corner
- [x] Shell: bar, quick settings and notifications, launcher (with every Settings page), Alt + Tab, snapping, lock screen, pop-ups, calendar, shortcuts sheet, clipboard history, emoji, screenshots and recording, password prompts
- [x] Settings, ours: Wi-Fi, network and VPN, Bluetooth, sound, display, appearance, power, AI, notifications, apps (with Flathub search), keyboard, mouse and touchpad, printers, date and time, language, accounts, privacy, accessibility, storage, updates (download only, never a surprise restart), about
- [~] Dictation: Super + M, speak, press again; whisper.cpp (pinned tag, built in the image) with a pinned multilingual base model, all on the device; Settings → Dictation (on/off). Not yet checked on a booted image
- [x] Every app in our style: GTK 3 and 4 colours, a Tripppin icon theme (green folders), Qt through qt6ct, Flatpak overrides, a text colour scheme
- [x] Apps: Thunar, Text Editor, Loupe, Papers, Calculator, Celluloid, File Roller, System Monitor, Disks; Firefox on first boot
- [x] Behaviour: USB automount, low battery, updates staged with rollback (greenboot), crash notices, the model freed first when memory runs low, a welcome tour
- [x] Polish pass: sharp text at a whole-number scale, a compact layout, icons throughout, a movable orb, spring motion everywhere (DESKTOP.md, "Polish pass")
- [x] Rectangle-style snapping (drag to edges and corners with a preview, Ctrl + Alt shortcuts, cycling thirds, restore) and desktop widgets the AI can write ([WIDGETS.md](WIDGETS.md))
- [x] Laptop round 3: Wi-Fi switch shows the real radio state at once; no focus ring trailing clicks; Corners reshapes every GTK window (Sharp is square), niri's focus ring neutral
- [x] Tiles direction (apps): Settings is a hub (SETTINGS label, big light pivots network / display / sound / power / ai / dictation / privacy / apps / about that slide and scroll sideways, a two-column grid of coloured rows with the state under each and flat switches for Wi-Fi and Bluetooth; a row slides in the existing page; search filters across pivots); glyphs are vectors drawn at device scale (`glyph.rs`)
- [x] Calendar app (`tripppin-ui calendar`, entry Calendar): day, week and month in the same style; "+ add call mum friday 6pm" is read by a local parser (no AI needed); events are one .ics file each in ~/.local/share/tripppin/calendar/ for the start screen's tile to read; reminders by notification at event time from a user systemd timer (tripppin-calendar-remind.timer, every minute, works with the app closed) and, while the app is open, its own 30 s check (de-duplicated). `calendar --popover` is the old small drop-down (not bound to Plasma's clock)
- [x] Terminal: on a terminal 100 columns or wider the AI's steps are a right-hand column of flat blocks (done green, running amber with marching dots, next dim, token line at the foot); narrower keeps the inline checklist. the Konsole profile and colour scheme match (DM Mono, #0b0e11, white block cursor, accent #3fae7a)
- [ ] Not yet: Tiles checked on the VM and on a real pointer; the AI as a fallback for calendar text the parser can't read; a Calendar tile on the start screen (reads the .ics folder)
- [x] A clean app launcher: helpers and duplicates of Settings pages hidden, "Terminal" and "Files", a quieter card
- [x] Design system: sentence case, readable type, tokens with a quiet green accent, a Sharp / Default / Round corners setting, a real Settings title bar, the computer called "tripppin" (UX-REVIEW.md)
- [x] The sparkle burst is gone everywhere (the mark keeps its upright spark); logging in goes from the login screen to a logo splash with no text console in between (DESKTOP.md, "The mark and the way in")
- [x] Laptop fixes from the first test: the red lock screen (root cause and a self-restarting lock), icons-only bar, a battery ring, the logo opening the apps, quick settings as one pill with working on/off tiles (DESKTOP.md, "Laptop fixes")
- [ ] Checked on the laptop: Wi-Fi, Bluetooth, sound devices, brightness, battery, the boot splash, a real USB stick and printer
- [ ] Later: light mode, sounds, voice input, on-screen keyboard (see DESKTOP.md)

## M8: Installer (later: after the OS is solid)

For now the ISO uses Fedora's Anaconda in interactive mode. The owner wants our own flow; parked until the OS itself is in good shape.

- [ ] `trip install`: a full-screen installer in our design (sentence case, Inter, forest green as an accent, the logo intro as the welcome)
- [ ] Flow: welcome, language and keyboard, Wi-Fi, disk (one clear warning), account, install with progress, done
- [ ] Engine: `bootc install to-disk`; our app is only the front end
- [ ] ISO that boots into our installer instead of Anaconda (image-builder's installer ISO type)
- [ ] First boot: offer the model download, so `/setup` isn't needed
- [ ] Later, a graphical version once the desktop exists (M5)
- [ ] Meanwhile, optionally brand Anaconda (product.img: logo, colours, name)

## Open questions for the owner

1. **Signing key:** OK to generate a cosign key pair and store the private key as a GitHub secret?
2. **ISO hosting:** Cloudflare R2 bucket (free egress) or another store?
3. **Branch protection:** turn it on now, or once a second contributor joins?
4. **Repo visibility:** private repos get 2,000 free Actions minutes a month. Nightly image builds use roughly 15–25 minutes each. Going public later makes standard runners free.
- [~] Right-click desktop menu and ~/Desktop icons (`tripppin-ui desktop`, see docs/DESKTOP.md); not yet checked in a session

## Plasma switch (2026-11-25, branch `plasma`)

- [x] Plasma 6 package set in os/Containerfile (explicit, not @kde-desktop); niri, waybar, swaync, fuzzel, wlogout, swaybg, swayidle, Thunar, blueman, wdisplays, pavucontrol, foot, mate-polkit, cliphist, qt5ct/qt6ct, grim/slurp/swappy/wf-recorder, udiskie removed with their config
- [x] Look-and-feel package `in.trippp.desktop`, colour scheme, Plasma style, Konsole profile, /etc/xdg defaults, shortcut map (docs/DESKTOP.md)
- [x] Login: our greeter on KWin; session starts `startplasma-wayland`
- [~] Checked in the VM by screenshot (docs/DESKTOP.md, Plasma section)
- [ ] To remove from crates/ui once Plasma is accepted: `displays`, `splash`, `desktop`, `dim`, `lock`, `keeper`, `snap`, `switch`, `osd`, `quick`, `battery-ring`, the niri-based Settings pages (Wi-Fi, Bluetooth, Sound, Display, Appearance, Power, Notifications, Keyboard, Mouse, Printers, Accessibility, now `#[allow(dead_code)]`), `keys` (cheat sheet lists niri keys), and the `foot` shim in os/files/usr/bin once orb.rs launches Konsole
