#!/usr/bin/env bash
# Builds the x86_64 installer ISO on this machine, for quick tests on real
# hardware. CI still makes the signed, scanned releases.
#
# Needs podman with a rootful connection. On Apple Silicon, use a podman
# machine with Rosetta (containers.conf: [machine] rosetta = true) so x86_64
# runs at near-native speed:
#   podman machine init --rootful --cpus 8 --memory 8192 tripppin-build
#
# Output: out/tripppin-local-x86_64.iso
set -euo pipefail
cd "$(dirname "$0")/.."

BIB=quay.io/centos-bootc/bootc-image-builder:latest
IMAGE=localhost/tripppin:local
started=$SECONDS

echo "== building the OS image"
podman build --platform linux/amd64 -f os/Containerfile -t "$IMAGE" .

echo "== building the installer ISO"
rm -rf out/bib && mkdir -p out/bib
podman run --rm --privileged --platform linux/amd64 --pull=newer \
  --security-opt label=type:unconfined_t \
  -v "$PWD/os/disk/iso.toml:/config.toml:ro" \
  -v "$PWD/out/bib:/output" \
  -v /var/lib/containers/storage:/var/lib/containers/storage \
  "$BIB" --type anaconda-iso --rootfs btrfs "$IMAGE"

iso=out/tripppin-local-x86_64.iso
mv out/bib/bootiso/install.iso "$iso"
rm -rf out/bib

# Same guard as CI: refuse an ISO that would install unattended.
if bsdtar -xOf "$iso" osbuild.ks | grep -qE "^(clearpart|autopart|rootpw|reboot)"; then
  echo "error: the ISO's kickstart would install unattended" >&2
  exit 1
fi
echo "== done in $(( (SECONDS - started) / 60 )) min: $iso"
