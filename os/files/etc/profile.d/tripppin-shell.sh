# Make the AI shell the default for interactive terminals. Inside it, /terminal
# opens a normal shell. Scripts, `ssh host command` and non-terminal sessions are
# untouched. Set TRIPPPIN_CLASSIC=1 to skip it entirely.
case $- in *i*) ;; *) return 0 ;; esac
[ -t 0 ] && [ -t 1 ] || return 0
[ -n "$TRIPPPIN_IN_SHELL$TRIPPPIN_CLASSIC" ] && return 0
command -v trip >/dev/null 2>&1 || return 0
export TRIPPPIN_IN_SHELL=1
# Leaving the AI shell normally ends the session; if it fails to start, fall back to a normal shell.
trip shell && exit
