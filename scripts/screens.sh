#!/usr/bin/env bash
# Headless screenshots of every Tripppin screen, for design review: a fresh tripppin-ui and
# this checkout's configs (waybar, quick settings, launcher, power menu) run in the OS image
# under sway's headless backend, and grim saves each screen. No VM, no GPU.
#
#   scripts/screens.sh [out-dir] [image]
#
# Needs podman and the OS image (default localhost/tripppin:dev, built from os/Containerfile).
# sway and grim are added to a throwaway layer on top of it the first time.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=${1:-out/screens}
BASE=${2:-localhost/tripppin:dev}
SHOTS=localhost/tripppin-shots:dev
mkdir -p "$OUT" out

if ! podman image exists "$SHOTS"; then
    printf 'FROM %s\nRUN dnf -y install --setopt=install_weak_deps=False sway grim && dnf clean all\n' "$BASE" |
        podman build -t "$SHOTS" -f - .
fi

# tripppin-ui for the image's architecture, built in a Fedora container like CI does.
# The devtools image (GTK headers preinstalled) when it's there, else plain Fedora.
BUILDER=localhost/tripppin-devtools:45
podman image exists "$BUILDER" || BUILDER=quay.io/fedora/fedora:45
podman run --rm -v "$PWD:/src" -v cargo-home:/root/.cargo -w /src "$BUILDER" sh -c '
  command -v cargo >/dev/null || dnf -y -q install --setopt=install_weak_deps=False cargo rust gcc gtk4-devel gtk4-layer-shell-devel >/dev/null 2>&1
  CARGO_TARGET_DIR=/root/.cargo/target-screens cargo build -q --release -p tripppin-ui
  cp /root/.cargo/target-screens/release/tripppin-ui /src/out/tripppin-ui-screens'

podman run --rm -e SCREENS -e CORNERS -v "$PWD:/src:ro" -v "$PWD/$OUT:/out" "$SHOTS" bash /src/scripts/screens-inside.sh
echo "screens in $OUT"
