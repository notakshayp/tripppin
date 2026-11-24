#!/usr/bin/env bash
# Runs inside the screenshots container (scripts/screens.sh): sway headless at 1280 x 800,
# this checkout's configs over the image's, then each screen in turn, saved by grim to /out.
# SCREENS="settings-wifi orb ..." limits it to some screens; CORNERS=sharp|round picks the
# corner style first (as Settings → Appearance does).
set -uo pipefail
export XDG_RUNTIME_DIR=/tmp/xdg HOME=/tmp/home USER=tripppin
export XDG_CONFIG_HOME=$HOME/.config XDG_STATE_HOME=$HOME/.local/state
mkdir -p -m 700 "$XDG_RUNTIME_DIR" && mkdir -p "$HOME/.config/tripppin"
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 GSK_RENDERER=cairo
export GTK_A11Y=none NO_AT_BRIDGE=1

# This checkout's files over the image's: the binary, bar, quick settings, launcher, menus.
install -m 755 /src/out/tripppin-ui-screens /usr/bin/tripppin-ui
cp -r /src/os/files/etc/xdg/. /etc/xdg/
cp -r /src/os/files/etc/wlogout/. /etc/wlogout/
cp -r /src/os/files/etc/skel/.config/. "$HOME/.config/"
cp /src/os/files/etc/gtk-4.0/settings.ini /etc/gtk-4.0/settings.ini 2>/dev/null || true
# The desktop asks for dark through the settings portal, which isn't running here.
echo "gtk-application-prefer-dark-theme=1" >> /etc/gtk-4.0/settings.ini
[ -n "${CORNERS:-}" ] && tripppin-ui corners "$CORNERS"

# sway carries file capabilities a rootless container can't grant; a plain copy drops them.
cp /usr/bin/sway /tmp/sway
cat > /tmp/sway.conf <<'EOF'
output HEADLESS-1 resolution 1280x800 bg /usr/share/tripppin/wallpaper.png fill
default_border none
default_floating_border none
for_window [app_id=".*"] floating enable
for_window [app_id="in.trippp.Settings"] resize set 920 600, move position center
for_window [app_id="in.trippp.Welcome"] move position center
for_window [app_id="in.trippp.FirstBoot"] move position center
EOF

run() { dbus-run-session -- bash -c "$1"; }
inner() {
    /tmp/sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
    for _ in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
    export WAYLAND_DISPLAY=wayland-1
    SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -1); export SWAYSOCK
    want() { [ -z "${SCREENS:-}" ] || [[ " $SCREENS " == *" $1 "* ]]; }
    shot() { sleep "${2:-2}"; grim "/out/$1.png" && echo "shot $1"; }
    reset() { pkill -f "tripppin-ui" ; pkill -x waybar; pkill -x swaync; pkill -x fuzzel; pkill -x wlogout; sleep 0.5; }

    for page in wifi network bluetooth sound display appearance power ai notifications apps keyboard pointer printers time language accounts privacy accessibility storage updates about; do
        want "settings-$page" || continue
        tripppin-ui settings "$page" >/dev/null 2>&1 &
        shot "settings-$page" 3
        reset
    done
    if want settings-narrow; then
        tripppin-ui settings appearance >/dev/null 2>&1 &
        sleep 2; swaymsg '[app_id="in.trippp.Settings"] resize set 640 480' >/dev/null
        shot settings-narrow 1; reset
    fi
    if want settings-wide; then
        tripppin-ui settings sound >/dev/null 2>&1 &
        sleep 2; swaymsg '[app_id="in.trippp.Settings"] resize set 1240 760, move position 20 20' >/dev/null
        shot settings-wide 1; reset
    fi
    if want bar; then waybar >/dev/null 2>&1 & shot bar 3; reset; fi
    if want quick-settings; then
        swaync >/dev/null 2>&1 & sleep 2
        notify-send -a Files "Copied 3 files" "Everything is in Documents now."
        swaync-client -op >/dev/null; shot quick-settings 2; reset
    fi
    if want notification; then
        swaync >/dev/null 2>&1 & sleep 2
        notify-send -a Updates "An update is ready" "It starts the next time you restart."
        shot notification 1; reset
    fi
    if want launcher; then fuzzel >/dev/null 2>&1 & shot launcher 2; reset; fi
    if want power-menu; then wlogout -b 5 -L 150 -R 150 -T 320 -B 320 >/dev/null 2>&1 & shot power-menu 2; reset; fi
    if want orb; then tripppin-ui orb >/dev/null 2>&1 & shot orb 3; fi
    if want ask; then
        pgrep -f "tripppin-ui orb" >/dev/null || { tripppin-ui orb >/dev/null 2>&1 & sleep 2; }
        tripppin-ui orb --toggle >/dev/null 2>&1; shot ask 2
    fi
    reset
    if want lock; then tripppin-ui lock --preview >/dev/null 2>&1 & shot lock 3; reset; fi
    if want greeter; then tripppin-ui greeter >/dev/null 2>&1 & shot greeter 3; reset; fi
    if want welcome; then tripppin-ui welcome >/dev/null 2>&1 & shot welcome 3; reset; fi
    if want first-boot; then tripppin-ui first-boot >/dev/null 2>&1 & shot first-boot 3; reset; fi
    if want askpass; then
        TRIPPPIN_SUDO_FOR="dnf install -y vlc" tripppin-ui askpass >/dev/null 2>&1 & shot askpass 2; reset
    fi
    if want osd; then tripppin-ui osd volume >/dev/null 2>&1 & shot osd 1; reset; fi
    if want calendar; then tripppin-ui calendar >/dev/null 2>&1 & shot calendar 2; reset; fi
    if want keys; then tripppin-ui keys >/dev/null 2>&1 & shot keys 2; reset; fi
    if want switch; then
        tripppin-ui settings >/dev/null 2>&1 & sleep 2
        tripppin-ui switch >/dev/null 2>&1 & shot switch 2; reset
    fi
}
export -f inner
export SCREENS="${SCREENS:-}"
run inner
