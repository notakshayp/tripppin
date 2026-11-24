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

Open a terminal and type what you want in plain words. A model running on your own machine works out the steps, checks itself, and does it. Every line you type goes to the AI; `/chat` holds a conversation, `/terminal` gives you a normal shell.

- **Built on Fedora bootc:** the whole OS is one signed image. Updates swap the image at reboot; a bad update rolls itself back.
- **Local first:** the model runs on your hardware, sized to it. Bring your own cloud key or server if you prefer.
- **Safe by construction:** the AI picks from typed tools rather than typing commands, and risky actions get a second check.
- **Rust:** everything we write.

## In the terminal

Logging in on a text console plays the logo: the ring draws itself, the × spins in, and a short system summary appears beside it. Run it any time with `trip logo`. (Logging in to the desktop plays the same intro as a full-screen splash instead.)

<p align="center">
  <img src="brand/terminal-logo.png" alt="Six frames of the terminal logo animation: the ring drawing, the × spark spinning in and settling, and the final logo" width="720">
</p>

## Try it

Test builds are attached to the private [`nightly`](https://github.com/notakshayp/tripppin/releases/tag/nightly) pre-release: an x86_64 installer ISO and virtual disk, and an aarch64 virtual disk that runs natively in UTM on Apple Silicon.

```bash
gh release download nightly --repo notakshayp/tripppin --pattern '*aarch64*'
```

Files over 2 GB are split into `.part` files; join them with `cat file.part* > file` before use. Checksums are in `SHA256SUMS-<arch>`, signed with the key in [`cosign.pub`](cosign.pub).

## Layout

```
crates/
  hwdetect/   reads memory, CPU and GPUs; picks the model tier
  trip/       the `trip` command: hw, logo, and more as milestones land
models/
  catalog.toml   which model each hardware tier gets
os/
  Containerfile  the OS image: Fedora bootc 45 + our tools
  files/         files copied into the image (login hooks, services)
  disk/          settings for ISO and disk image builds
.github/workflows/
  ci.yml      fmt, clippy, tests on x86_64 and arm64; licences and advisories
  image.yml   builds, scans, signs and publishes the OS image (x86_64 + arm64)
  disk.yml    turns the image into an ISO and virtual disks on the nightly release
brand/        the logo (static, theme-aware, animated), avatars, terminal logo frames
docs/         plan and research notes
site/         plan pages (static HTML)
```

## Develop

Needs Rust (stable, 1.85 or newer).

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

## Build the OS image

Needs podman, run as root because bootc tooling needs rootful storage.

```bash
sudo podman build -f os/Containerfile -t localhost/tripppin .
sudo podman run --rm localhost/tripppin trip logo --static
```

CI builds the same image for x86_64 and arm64 on every PR that touches `os/`, `crates/` or `models/`, scans it for vulnerabilities, and on `main` and nightly publishes and signs `ghcr.io/notakshayp/tripppin:nightly`.

## Brand

| File | Use |
|---|---|
| [`brand/logo.svg`](brand/logo.svg) | The mark in ink, for light backgrounds |
| [`brand/logo-themed.svg`](brand/logo-themed.svg) | Switches between light and dark with the viewer's theme |
| [`brand/logo-animated.svg`](brand/logo-animated.svg) | The intro: the ring draws itself, the × spins in and settles, then turns a quarter now and then |
| [`brand/social/`](brand/social) | 1024×1024 avatars: dark, light, green |

The power symbol with a spark where its line would be: switch it on, and there's intelligence inside. The spark is an ×, two arrows meeting in the middle, sitting in the ring's opening. One colour (ink or paper) with a green spark. Don't outline it, add effects or separate the spark from the ring. Every raster (social images, app icon, wallpapers, boot splash, terminal frames) comes from `scripts/brand-assets.sh`.

## Rules

- Everything we write is Rust. Use a proven library when it's the best implementation; don't glue whole apps together.
- Keep code Apache-2.0 compatible. Don't link GPL libraries; call GPL tools as separate processes.
- Pin everything: base image digests, toolchains, model files. Updates arrive as PRs.
- Every PR keeps `ci.yml` green: fmt, clippy `-D warnings`, tests and `cargo deny`.
- Update `docs/PLAN.md` in the same PR as the work it tracks.

## Licence

Apache-2.0. See [`LICENSE`](LICENSE).
