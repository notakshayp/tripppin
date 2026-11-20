# Show the animated tripppin logo once per login, on interactive terminals only.
# Set TRIPPPIN_NO_LOGO=1 to turn it off.
case $- in *i*) ;; *) return 0 ;; esac
[ -t 1 ] || return 0
[ -n "$TRIPPPIN_LOGO_SHOWN" ] && return 0
[ -n "$TRIPPPIN_NO_LOGO" ] && return 0
export TRIPPPIN_LOGO_SHOWN=1
command -v trip >/dev/null 2>&1 && trip logo
