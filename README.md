<p align="center">
  <img src="brand/logo-animated.svg" alt="Tripppin logo: a power symbol with a spark where its line would be" height="150">
</p>

<h1 align="center">tripppin</h1>

<p align="center">
  <b>Tell your computer what you want. It does it.</b><br>
  A Linux operating system with an AI built in.
</p>

<p align="center">
  <a href="https://trippp.in">trippp.in</a> ·
  <a href="docs/PLAN.md">plan</a> ·
  <a href="site/roadmap.html">build plan page</a> ·
  <a href="https://github.com/notakshayp/tripppin-web">website source</a>
</p>

<p align="center">
  <a href="https://github.com/notakshayp/tripppin/actions/workflows/ci.yml"><img src="https://github.com/notakshayp/tripppin/actions/workflows/ci.yml/badge.svg" alt="ci"></a>
  <a href="https://github.com/notakshayp/tripppin/actions/workflows/image.yml"><img src="https://github.com/notakshayp/tripppin/actions/workflows/image.yml/badge.svg" alt="image"></a>
  <a href="https://github.com/notakshayp/tripppin/actions/workflows/disk.yml"><img src="https://github.com/notakshayp/tripppin/actions/workflows/disk.yml/badge.svg" alt="disk"></a>
</p>

---

## What it is

A small Linux distribution, based on Alpine, whose whole interface is one full-screen text UI (`trip home`) you can click and type into. Say what you want in plain words; a model running on your own machine works out the steps, shows each one, searches the web or installs what it needs, and asks before anything risky. GUI apps you install still open full screen.

- **Light:** about a 460 MB ISO, ~90 MB of memory at the home screen, ~0% CPU when idle.
- **Local first:** the model runs on your hardware, sized to it. Bring your own cloud key or server if you prefer.
- **Safe by construction:** the AI picks from typed tools rather than typing commands, and risky actions get a second check.
- **Rust:** everything we write.

## In the terminal

Logging in on a text console plays the logo: the ring draws itself, the spark spins in, and a short system summary appears beside it. Run it any time with `trip logo`. (Logging in to the desktop plays the same intro as a full-screen splash instead.)

<p align="center">
  <img src="brand/terminal-logo.png" alt="Six frames of the terminal logo animation: the ring drawing, the spark spinning in and settling, and the final logo" width="720">
</p>

## Try it

Test ISOs for x86_64 and aarch64 (runs natively in UTM on Apple Silicon) are on the [`nightly`](https://github.com/notakshayp/tripppin/releases/tag/nightly) pre-release and at trippp.in. Write the ISO to a USB stick and boot it; it starts straight into the TUI (user `trip`, password `tripppin`), and `trip-install` installs it to disk. Checksums are in `SHA256SUMS-<arch>`, signed with the key in [`cosign.pub`](cosign.pub). See [`alpine/README.md`](alpine/README.md).

## Layout

```
crates/
  hwdetect/   reads memory, CPU and GPUs; picks the model tier
  trip/       the `trip` command: hw, logo, and more as milestones land
models/
  catalog.toml   which model each hardware tier gets
alpine/
  packages/      our APKBUILDs (tripppin-base, trip, llama.cpp, whisper.cpp, firmware)
  mkimage/       the ISO profile and its initial /etc
  build-packages.sh, build-iso.sh, install.sh, qemu.sh
.github/workflows/
  ci.yml      fmt, clippy, tests on x86_64 and arm64; licences and advisories
  iso.yml     builds the x86_64 and aarch64 ISOs, signs them, nightly release + R2
brand/        the logo (static, theme-aware, animated), avatars, terminal logo frames
docs/         plan and research notes
site/         plan pages (static HTML)
```

## Develop

Needs Rust (stable, 1.89 or newer).

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo run -p trip -- logo        # works anywhere
cargo run -p trip -- hw          # Linux only; macOS prints an error
```

On macOS you can still lint the Linux-only code:

```bash
rustup target add x86_64-unknown-linux-gnu
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
```

## Build the OS

Needs podman. Packages, then the ISO, then boot it:

```bash
alpine/build-packages.sh aarch64
alpine/build-iso.sh aarch64
alpine/qemu.sh iso
```

CI builds both architectures on every push to `main` that touches `alpine/`, `crates/` or `models/`, and publishes them.

## Brand

| File | Use |
|---|---|
| [`brand/logo.svg`](brand/logo.svg) | The mark in ink, for light backgrounds |
| [`brand/logo-themed.svg`](brand/logo-themed.svg) | Switches between light and dark with the viewer's theme |
| [`brand/logo-animated.svg`](brand/logo-animated.svg) | The intro: the ring draws itself, the spark spins in and settles, then turns a quarter now and then |
| [`brand/social/`](brand/social) | 1024×1024 avatars: dark, light, green |

The power symbol with a spark where its line would be: switch it on, and there's intelligence inside. One colour (ink or paper) with a green spark. Don't outline it, add effects or separate the spark from the ring. Every raster (social images, app icon, wallpapers, boot splash, terminal frames) comes from `scripts/brand-assets.sh`.

## Rules

- Everything we write is Rust. Use a proven library when it's the best implementation; don't glue whole apps together.
- Keep code Apache-2.0 compatible. Don't link GPL libraries; call GPL tools as separate processes.
- Pin everything: base image digests, toolchains, model files. Updates arrive as PRs.
- Every PR keeps `ci.yml` green: fmt, clippy `-D warnings`, tests and `cargo deny`.
- Update `docs/PLAN.md` in the same PR as the work it tracks.

## Licence

Apache-2.0. See [`LICENSE`](LICENSE).
