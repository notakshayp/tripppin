#!/usr/bin/env bash
# Ships the locally built image (localhost/tripppin:dev) to the UTM test VM and reboots
# it into that image. Each copy gets a unique name, so bootc sees a new image.
set -euo pipefail
cd "$(dirname "$0")/.."
VM=${VM:-192.168.64.3}
PASS=$(awk '/password/{print $2}' "$HOME/.config/tripppin/test-login")
SSH_OPTS=(-i "${KEY:-$HOME/.ssh/google_compute_engine}" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$PWD/out/vm-known-hosts" -o ConnectTimeout=5)
name="tripppin-$(date +%s).tar"
rm -f out/tripppin-dev.tar
podman save --format oci-archive -o out/tripppin-dev.tar localhost/tripppin:dev
ssh "${SSH_OPTS[@]}" "tripppin@$VM" 'rm -f /var/tmp/tripppin-*.tar'
scp -q "${SSH_OPTS[@]}" out/tripppin-dev.tar "tripppin@$VM:/var/tmp/$name"
ssh "${SSH_OPTS[@]}" "tripppin@$VM" "echo '$PASS' | sudo -S -p '' sh -c 'bootc switch --transport oci-archive /var/tmp/$name >/dev/null && systemctl reboot'" || true
sleep 25
until ssh "${SSH_OPTS[@]}" "tripppin@$VM" true 2>/dev/null; do sleep 4; done
echo "VM rebooted into $name"
