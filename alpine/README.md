# Tripppin on Alpine

Tripppin is a minimal Alpine Linux system with no desktop. The whole interface is a full-screen TUI (`trip home`) running in one terminal inside an invisible Wayland compositor, so text is crisp, colour is truecolor, the mouse works, and GUI apps you install can open full screen on top.

Priorities, in order: speed on low-power devices, small image, a strong AI harness.

## What is in the image

| Part | Choice | Why |
|---|---|---|
| Base | Alpine 3.24.4 (musl, OpenRC, busybox), pinned by container digest and aports commit | smallest sane base; boots in seconds |
| Kernel | `linux-lts` 6.18 | long-term support, small, supports 2025 laptops; `linux-stable` (7.x) is one line away in `mkimage/mkimg.tripppin.sh` if a new machine needs it |
| Compositor | `sway`, stripped config (`packages/tripppin-base/sway.conf`) | `cage` runs one app only; sway gives Super+Tab switching with no bars, borders or titles |
| Terminal | `foot` | fast, Wayland-native, truecolor, sixel, SGR mouse; the font and colours are ours |
| Font | JetBrains Mono, DejaVu Sans Mono as fallback (braille, symbols) | a few MB in total |
| Seat/session | `seatd` (no elogind, no PAM session stack) | lightest thing sway needs |
| Network | `iwd` (Wi-Fi) + `dhcpcd` + `openresolv`, `chrony` | no NetworkManager |
| Audio | `pipewire` + `wireplumber` + `pipewire-pulse` | speakers, mic for dictation, Bluetooth audio via `bluez` |
| GPU | `mesa` Vulkan: intel (x86 only), radv, virtio, lavapipe; dri gallium | |
| Firmware | subset: amdgpu, i915, xe, intel (iwlwifi, SOF audio), ath10k/11k/12k, rtlwifi, rtw88/89, mediatek, brcm, qca, rtl_bt | not the whole linux-firmware |
| AI | `llama.cpp` (Vulkan, runtime CPU dispatch), `whisper.cpp` (CPU), `trip` | built from pinned tags; models are downloaded on first boot by `trip` from `models/catalog.toml` |
| Extras | `doas`, `openssh` (off by default), `tzdata`, `ca-certificates`, `curl`, `btrfs-progs`, `brightnessctl` | |

Session: autologin on tty1 (busybox `getty -l`) logs in the user `trip`; `/etc/profile.d/tripppin.sh` starts `sway`; sway starts the audio stack and `foot --app-id trip-home` running `trip home`. If `trip` exits you land in a shell inside the same terminal.

Keys: `Super+Tab` next app, `Super+Shift+Tab` previous, `Super+Esc` home, `Super+Q` close the focused app (never home). Every window is forced full screen. Brightness and volume keys work. Local sway overrides go in `/etc/tripppin/sway.d/*.conf` (HiDPI scale, keyboard layout).

Default login: user `trip`, password `tripppin` (live ISO, and the installer asks for a new one). `doas` asks for that password.

## Build

Needs podman with a rootful connection (`PODMAN_CONNECTION`, default `tripppin-build-root`). On an Apple Silicon Mac the podman machine is arm64, so aarch64 builds natively; x86_64 builds run under emulation (slow), CI builds x86_64 natively.

```sh
alpine/build-packages.sh aarch64      # tripppin-base, trip, llama-cpp-vulkan, whisper-cpp -> alpine/out/repo/aarch64
alpine/build-iso.sh aarch64           # hybrid UEFI ISO -> alpine/out/iso/aarch64/
```

`build-packages.sh` accepts package names to rebuild just those (`alpine/build-packages.sh aarch64 tripppin-base`). The first build of `trip` and llama.cpp takes a while (about 10 and 25 minutes on an M-series podman VM); the cargo registry is cached in a podman volume.

Pins: the builder container is `alpine@sha256:1829...` (3.24.4) and aports is checked out at the `v3.24.4` release commit (`alpine/build/Containerfile`). llama.cpp `v1.0.0` and whisper.cpp `v1.9.6` are pinned by sha512 in their APKBUILDs. Upstream reference: `alpine-standard-3.24.4-aarch64.iso` sha256 `c0e2e9a8...` (we do not use it, only mkimage).

### Signing

The apk repository and the ISO's modloop are signed. Locally `build-packages.sh` generates a throwaway RSA key in `alpine/out/keys/` (git-ignored; never commit a private key). In CI set `ABUILD_PRIVKEY=/path/to/key` from a secret; the matching public key is installed as `/etc/apk/keys/tripppin.rsa.pub` on the image. `alpine/keys/` is reserved for the real public key once it exists.

## Test in QEMU (Apple Silicon)

```sh
brew install qemu
alpine/qemu.sh iso          # VNC on localhost:5901, monitor socket alpine/out/qemu-mon.sock, serial in alpine/out/serial.log
echo "screendump /tmp/shot.ppm" | nc -U alpine/out/qemu-mon.sock
```

## Install to disk

From the live ISO, as `trip` (the installer ships as `/usr/share/tripppin/install.sh`, `trip install` will wrap it later):

```sh
doas sh /usr/share/tripppin/install.sh /dev/vda
```

It erases the disk, makes an EFI partition and a btrfs partition with subvolumes `@` (root, including `/boot`) and `@snapshots` (mounted at `/.snapshots`), installs the packages from the ISO, and sets up GRUB (UEFI, removable path). `trip update` will snapshot `@` into `@snapshots` before `apk upgrade`; booting an older snapshot is the rollback (not implemented yet).

## Layout

```
alpine/
  build/Containerfile        pinned builder (alpine + abuild + aports mkimage)
  build-packages.sh          abuild the packages in the builder
  build-iso.sh               mkimage the ISO
  mkimage/                   mkimg.tripppin.sh profile and genapkovl (the live /etc)
  packages/                  APKBUILDs: tripppin-base, trip, llama-cpp-vulkan, whisper-cpp
  install.sh                 disk installer (UEFI, btrfs)
  qemu.sh                    QEMU/HVF runner for the Mac
```

## Not done yet

- A login screen inside the TUI, `trip install`, `trip update` and snapshot rollback.
- Real signing key and a hosted apk repository (`pkgs.trippp.in`); CI jobs.
- x86_64 build verified in CI (the aarch64 build is what has been run here).
- Retiring the Fedora/Plasma image (`os/`, `crates/ui`).
