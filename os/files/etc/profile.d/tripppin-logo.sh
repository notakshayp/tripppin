# Show the animated tripppin logo once per login, on interactive terminals only.
# Set TRIPPPIN_NO_LOGO=1 to turn it off.
case $- in *i*) ;; *) return 0 ;; esac
[ -t 1 ] || return 0
[ -n "$TRIPPPIN_LOGO_SHOWN" ] && return 0
[ -n "$TRIPPPIN_NO_LOGO" ] && return 0
# Never on the way into the desktop: tty1 is the login screen's and the desktop's (greetd),
# where the splash shows the logo instead. Every terminal window on the desktop plays it.
case "$(tty)" in /dev/tty1) return 0 ;; esac
export TRIPPPIN_LOGO_SHOWN=1
command -v trip >/dev/null 2>&1 || return 0
# kmscon gives logins a pseudo-terminal and draws 24-bit colour; tell programs so.
case "$(tty)" in /dev/pts/*) [ -z "$SSH_CONNECTION" ] && pgrep -x kmscon >/dev/null 2>&1 && export COLORTERM=truecolor ;; esac
# Start from a clean screen: the login prompt and boot messages go.
printf '\033[H\033[2J\033[3J'
trip logo
