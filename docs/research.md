# Tripppin research notes

Collected 2026-11-20. Vendor-reported numbers are marked as such; our own eval suite decides final choices.

## Product decisions so far

- Audience: people who don't want to do the technical lifting, power users included.
- Plain text in the terminal always goes to the model. `!cmd` runs a raw command; `/term` opens a classic shell.
- Everything we write is Rust. Proven dependencies are used where they're the best implementation.
- Minimum 16 GB memory for local AI. Below that: cloud key or the user's own OpenAI-compatible server.
- Safety: an AI verifier reviews risky actions, with hard-coded rules underneath. YOLO mode keeps two exceptions (shell commands and anything prompted by outside content still go through the verifier).
- Web search: SearXNG running locally, plus a curl-style fetch with optional headless browser.
- Graphical installer: our own, in Rust (iced), using Inter and DM Mono.
- Distribution: open source on GitHub first. Selling is out of scope for now. No other distro's name in branding.
- Domain: trippp.in. Product name: Tripppin.
- Base system: still being decided. Leading candidate is an image-based Fedora (bootc); see below.

## Models (tool calling)

BFCL v4 overall (hard multi-step agent tasks; a top cloud model scores ~77):

| Model | Score | License |
|---|---|---|
| Qwen3.5 4B / 9B / 27B | 50 / 66 / 69 (vendor) | Apache-2.0 |
| Qwen3.5 35B-A3B (MoE) | 67 (vendor) | Apache-2.0 |
| Qwen3 8B / 14B / 32B | 43 / 41 / 49 | Apache-2.0 |
| Gemma 4 (E2B/E4B/26B MoE/31B) | not on BFCL; τ²-bench E4B 42, 31B 77 | Apache-2.0 |
| Llama 3.x / 4 | 22–28 | Llama license (conditions) |
| xLAM-2, Hammer | 41–55 | Non-commercial: can't ship |

Gemini is closed and cloud-only; usable only through a user's own key.

Hardware tiers: 16 GB no GPU → Qwen3.5 4B; 8 GB+ VRAM → 9B; 32 GB no GPU → 35B-A3B; 24 GB+ VRAM → 27B.

## Making small models reliable

- Route with rules or a small classifier, not the big model; fixed tool subset (5–15) and fixed prompt per category so the prompt cache holds.
- Constrain with llama.cpp's lazy grammar in each model's native tool-call format (Qwen3.5 uses XML, not JSON). Always allow a plain-text reply.
- Validate arguments against reality, retry at most twice with the error.
- Two passes when a structured final answer is needed (tools first, then format).
- Thinking off for single actions; capped thinking for plans on GPU tiers.
- Token budget per component; trim tool output; stream; Ctrl-C cancels.
- KV cache: unified (`--kv-unified`) when using parallel slots; don't quantize below 8 bits.
- Harness supervises llama-server and kills it on idle rather than relying on its sleep mode (known wake bugs).
- Hybrid models (Qwen3.5 Gated-DeltaNet) have open llama.cpp issues with prompt reuse — benchmark before relying on cache reuse.

## Runtime and Rust stack

- llama.cpp server v1.0 (2026-11-13): broadest GPU support (CUDA, ROCm, Vulkan, SYCL, Metal, CPU), grammars, slots, idle sleep. C API is the only stability promise, so pin builds. Keep its built-in `--tools/--agent/--mcp-servers-config` off.
- mistral.rs (pure Rust) lacks ROCm/Vulkan/Intel; candle is a framework, not a server. Keep a backend trait for later.
- Crates: rmcp (official MCP SDK), reedline, ratatui, iced / iced_layershell, landlock, seccompiler, zbus, llguidance.
- Avoid the `alpm` crate (GPL-3.0); call pacman/expac or rpm as subprocesses to keep our code Apache-2.0.
- Compositor: Hyprland (BSD-3, most configurable, fast-moving: pin and own its library stack). Layer-shell supported by Hyprland, Sway, KDE, niri, COSMIC; not GNOME.

## Bill of materials

`trip bom` design: list / outdated / audit / crates / licenses / export (SPDX, CycloneDX) / diff. Build Rust with cargo-auditable; generate SBOMs with syft at build time; license gate in the image build.

## Base system options

| Base | Notes |
|---|---|
| Fedora bootc (image-based) | Leading candidate. Whole OS built as a signed container image in CI; machines update atomically with automatic rollback (greenboot); SELinux; read-only core; Flatpak apps; proven by Bazzite/Bluefin. |
| openSUSE MicroOS | Strong second; transactional updates, smaller community. |
| Arch (staged) | Lightest, but rollback, staging, keyring and Hyprland rebuilds are all on us. |
| NixOS | Reproducible but adds a configuration language. |
| Debian / Alpine | Too slow for AI hardware / poor NVIDIA and desktop support. |

## Independent review (5 reviewers, no context)

Key fixes adopted: taint tracking for all outside content (not just web); egress limits on web.fetch; signed-repo-only tool auto-registration; narrow privileged helper with uncached polkit; honest undo table (home snapshots, irreversible actions labeled); sealed activity log; Unix sockets with peer checks; keys in Secret Service; signed model catalog with vetted chat templates; staged updates; partial-upgrade prevention; GPU driver handling; build infrastructure first; per-release GPL source archive; cargo-about notices; no GPL linking; per-model native tool formats; context budgets; streaming and cancel; scheduler priority for user requests; first-run tour; accessibility in v1.

## Infrastructure

- Firebase project `tripppin-os`; landing page at https://tripppin-os.web.app, custom domains trippp.in (+ www redirect).
- DNS at GoDaddy: A @ 199.36.158.100, TXT @ hosting-site=tripppin-os, CNAME www → tripppin-os.web.app.
