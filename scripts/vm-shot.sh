#!/usr/bin/env bash
# Screenshots the UTM test VM's screen through niri: the login screen if nobody is logged
# in, else the desktop of the first logged-in user. Saves to out/<name>.png.
set -euo pipefail
cd "$(dirname "$0")/.."
VM=${VM:-192.168.64.3}
NAME=${1:-shot}
PASS=$(awk '/password/{print $2}' "$HOME/.config/tripppin/test-login")
SSH_OPTS=(-i "${KEY:-$HOME/.ssh/google_compute_engine}" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$PWD/out/vm-known-hosts")
ssh "${SSH_OPTS[@]}" "tripppin@$VM" "echo '$PASS' | sudo -S -p '' sh -c '
  sock=\$(ls -t /run/user/*/niri*.sock 2>/dev/null | head -1)
  uid=\$(echo \$sock | cut -d/ -f4); who=\$(id -nu \$uid)
  rm -f /var/tmp/shot.png
  sudo -u \$who XDG_RUNTIME_DIR=/run/user/\$uid NIRI_SOCKET=\$sock niri msg action screenshot-screen -p false --path /run/user/\$uid/tripppin-shot.png
  sleep 1; cp /run/user/\$uid/tripppin-shot.png /var/tmp/shot.png; chmod 644 /var/tmp/shot.png'"
scp -q "${SSH_OPTS[@]}" "tripppin@$VM:/var/tmp/shot.png" "out/$NAME.png"
echo "out/$NAME.png"
