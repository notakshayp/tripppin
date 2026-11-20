# Tripppin build plan

The full picture, with diagrams, is in `site/roadmap.html`. This file is the working checklist: what's done, what's next and what's blocked. Update it in the same PR as the work.

## Decisions in force

| Area | Decision |
|---|---|
| Base | Fedora bootc 45, image-based. The whole OS is one signed OCI image; machines update atomically and roll back on failed boots. |
| Language | Rust for everything we write (edition 2024, MSRV 1.85). |
| Model engine | llama.cpp server as a supervised child process; Vulkan by default, NVIDIA image variant later. |
| Desktop | Hyprland, packaged and pinned by us (Fedora no longer ships it). Fallback: Sway. |
| Shell | Plain text goes to the model. `!cmd` runs a raw command; `/term` opens a classic shell. |
| Safety | Code is the boundary (taint labels, policy engine, sandbox); an AI verifier checks intent on top. YOLO keeps two exceptions. |
| Minimum | 16 GB memory for local AI; below that, a cloud key or the user's own server. |
| Registry | `ghcr.io/notakshayp/tripppin`, tags `nightly`, `<sha>`; later `testing`, `beta`, `stable`. |
| License | Apache-2.0 for our code. |
| Website | Separate repo: `notakshayp/tripppin-web` (trippp.in). |

## Milestones

| # | Milestone | You can try | Status |
|---|---|---|---|
| M0 | Foundations | CI on every PR | **in progress** |
| M1 | First boot (ISO 1) | Install, log into a Tripppin Hyprland desktop | **in progress** |
| M2 | Update loop | Laptop updates overnight, bad build rolls back | |
| M3 | Model service | `trip ask "hello"` answers locally | |
| M4 | AI shell (ISO 2) | Sentences in the terminal do things | |
| M5 | Safety | Booby-trapped pages get refused | |
| M6 | Desktop (ISO 3) | Daily driver for a week | |
| M7 | Installer | Five-minute install, our own UI | |
| M8 | Public beta | Download from trippp.in | |

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

## M1: First boot, next up

- [ ] Hyprland packaging: decide between building RPMs in our CI (mock + Packit) or a COPR; package hyprland, aquamarine, hyprutils, hyprlang, hyprcursor, hyprgraphics, hyprwayland-scanner, xdg-desktop-portal-hyprland
- [ ] Desktop session: greetd + a minimal greeter, PipeWire, NetworkManager, fonts (Inter), Flatpak with Flathub
- [ ] Branding: our os-release name and pretty name (keep `ID=fedora` so bootc-image-builder still works), Plymouth splash with the Glance mark, wallpaper, default Hyprland config (floating-first)
- [ ] greenboot health checks: session starts, network, our services
- [x] Multi-arch image: amd64 and arm64 built natively, joined under one tag, list and per-arch images signed
- [x] `disk.yml`: bootc-image-builder → x86_64 ISO + qcow2, aarch64 qcow2 (for UTM on the Mac); compressed, split under 2 GiB, checksums signed, attached to the `nightly` pre-release
- [x] Test login secrets (`TEST_USER_PASSWORD`, `TEST_USER_SSH_KEY`)
- [x] Console branding: os-release name, login banner with the logo, `trip logo` animation on login
- [ ] First disk build attached to the `nightly` release
- [ ] `boot-test.yml`: boot the qcow2 under KVM, wait for `multi-user.target`, check `trip hw` and `bootc status`

## Open questions for the owner

1. **Signing key:** OK to generate a cosign key pair and store the private key as a GitHub secret?
2. **ISO hosting:** Cloudflare R2 bucket (free egress) or another store?
3. **Branch protection:** turn it on now, or once a second contributor joins?
4. **Repo visibility:** private repos get 2,000 free Actions minutes a month. Nightly image builds use roughly 15–25 minutes each. Going public later makes standard runners free.
