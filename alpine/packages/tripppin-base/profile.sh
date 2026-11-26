# Tripppin: on the first console, start the compositor and the home terminal.
if [ "$(id -un)" = trip ] && [ "$(tty)" = /dev/tty1 ] && [ -z "$WAYLAND_DISPLAY" ] && [ -z "$TRIP_NO_SESSION" ]; then
    /usr/libexec/tripppin/session
fi
