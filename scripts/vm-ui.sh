#!/usr/bin/env bash
# Dev loop for the desktop screens: build tripppin-ui for ARM in a Fedora container,
# copy it into the UTM test VM over a writable /usr overlay, and restart it.
# Needs: a podman machine, the VM at $VM (default 192.168.64.3), the test login file.
set -euo pipefail
cd "$(dirname "$0")/.."
VM=${VM:-192.168.64.3}
KEY=${KEY:-$HOME/.ssh/google_compute_engine}
# The test VM's host key changes whenever it's recreated; keep it out of ~/.ssh/known_hosts.
SSH_OPTS=(-i "$KEY" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="${KNOWN_HOSTS:-$PWD/out/vm-known-hosts}")
PASS=$(awk '/password/{print $2}' "$HOME/.config/tripppin/test-login")
podman run --rm --platform linux/arm64 -v "$PWD:/src" -v cargo-home:/root/.cargo -w /src quay.io/fedora/fedora:45 sh -c '
  dnf -y -q install --setopt=install_weak_deps=False cargo rust gcc gtk4-devel gtk4-layer-shell-devel >/dev/null 2>&1
  CARGO_TARGET_DIR=/root/.cargo/target-linux cargo build -q --release -p tripppin-ui
  cp /root/.cargo/target-linux/release/tripppin-ui /src/out/tripppin-ui'
scp -q "${SSH_OPTS[@]}" out/tripppin-ui "tripppin@$VM:/var/tmp/tripppin-ui"
ssh "${SSH_OPTS[@]}" "tripppin@$VM" "echo '$PASS' | sudo -S -p '' sh -c '
  # /usr is the read-only image: a writable overlay (until reboot) takes the test build.
  # Never use dnf install --transient on the VM: it leaves the overlay read-only.
  bootc usr-overlay >/dev/null 2>&1 || true
  install -m 755 /var/tmp/tripppin-ui /usr/bin/tripppin-ui
  # Someone logged in: restart just the orb in their session. Otherwise, the login screen.
  sock=\$(ls /run/user/1000/niri*.sock 2>/dev/null | head -1)
  if [ -n \"\$sock\" ]; then
    pkill -f \"^(/usr/bin/)?tripppin-ui orb\" || true
    sudo -u \$(id -nu 1000) XDG_RUNTIME_DIR=/run/user/1000 NIRI_SOCKET=\$sock niri msg action spawn -- tripppin-ui orb
  else
    systemctl restart greetd
  fi'"
echo "tripppin-ui updated on $VM"
