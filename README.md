# tripppin

A Linux operating system with an AI built in. Tell your computer what you want; it plans, checks itself and gets it done.

- Website: https://trippp.in (source: [`notakshayp/tripppin-web`](https://github.com/notakshayp/tripppin-web))
- Plan: [`docs/PLAN.md`](docs/PLAN.md) is the checklist. [`site/roadmap.html`](site/roadmap.html) is the full picture, and [`site/index.html`](site/index.html) explains the idea.

## Layout

```
crates/
  hwdetect/   reads memory, CPU and GPUs; picks the model tier
  trip/       the `trip` command line (`trip hw` today)
models/
  catalog.toml   which model each hardware tier gets
os/
  Containerfile  the OS image: Fedora bootc 45 + our tools
.github/workflows/
  ci.yml      fmt, clippy, tests on x86_64 and arm64
  image.yml   builds the OS image; pushes ghcr.io/notakshayp/tripppin on main and nightly
brand/        logo files (Glance)
docs/         plan and research notes
site/         plan pages (static HTML)
```

## Develop

Needs Rust (stable, 1.85 or newer).

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
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
sudo podman run --rm localhost/tripppin trip --version
```

CI builds the same image on every PR that touches `os/`, `crates/` or `models/`. On `main` and nightly it pushes `ghcr.io/notakshayp/tripppin:nightly` and `:<sha>`.

Disk images, ISOs and boot tests arrive in milestone 1.

## Rules

- Everything we write is Rust. Use a proven library when it's the best implementation; don't glue whole apps together.
- Keep code Apache-2.0 compatible. Don't link GPL libraries; call GPL tools as separate processes.
- Pin everything: base image digests, toolchains, model files. Updates arrive as PRs.
- Every PR keeps `ci.yml` green: fmt, clippy `-D warnings` and tests.
- Update `docs/PLAN.md` in the same PR as the work it tracks.
