#!/bin/sh
# Generates the Tripppin icon theme's app icons (Bento style: a filled, saturated rounded square,
# 28% radius, with a bold dark 2.2 px glyph) into os/files/usr/share/icons/Tripppin/scalable/apps.
# Glyph paths are the design's (Bento mock section 08), on a 24 grid. Pure sh; no dependencies.
set -e
out="$(dirname "$0")/../os/files/usr/share/icons/Tripppin/scalable/apps"
rm -rf "$out"
mkdir -p "$out"
# glyph NAME COLOUR BODY  (BODY drawn on a 24x24 grid, scaled to the middle of a 256 square)
glyph() {
  cat > "$out/$1.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
<rect width="256" height="256" rx="72" fill="$2"/>
<g transform="translate(64 64) scale(5.3333)" fill="none" stroke="#16181a" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">$3</g>
</svg>
SVG
}
glyph terminal  '#f1f3f4' '<path d="M4 6l5 6-5 6M12 18h8"/>'
glyph files     '#5aa9e6' '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>'
glyph settings  '#aab3b8' '<path d="M4 6h9M17 6h3M15 4v4M4 12h3M11 12h9M9 10v4M4 18h11M19 18h1M17 16v4"/>'
glyph store     '#45c486' '<path d="M5 8h14l-1 12H6zM9 8a3 3 0 0 1 6 0"/>'
glyph calendar  '#ff6b6b' '<path d="M4 6h16v14H4zM4 10h16M8 3v4M16 3v4"/>'
glyph dictate   '#ff8a5a' '<path d="M9 4a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0zM5 11a7 7 0 0 0 14 0M12 18v3"/>'
cat > "$out/ask.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
<rect width="256" height="256" rx="72" fill="#f2b84b"/>
<path transform="translate(64 64) scale(5.3333)" d="M12 2l1.5 7.5L21 11l-7.5 1.5L12 20l-1.5-7.5L3 11l7.5-1.5z" fill="#16181a" stroke="#16181a" stroke-width="1" stroke-linejoin="round"/>
</svg>
SVG
cat > "$out/logo.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
<rect width="256" height="256" rx="72" fill="#f2b84b"/>
<g transform="translate(58.1 58.4) scale(0.7)"><path d="M58.32 70.84 A60 60 0 1 0 141.68 70.84" fill="none" stroke="#16181a" stroke-width="24" stroke-linecap="round"/><path d="M100 14 106.11 43.89 136 50 106.11 56.11 100 86 93.89 56.11 64 50 93.89 43.89Z" fill="#16181a"/></g>
</svg>
SVG
# Alias icon names the apps ask for, then drop the bases.
alias_to() { t=$1; shift; for n in "$@"; do cp "$out/$t.svg" "$out/$n.svg"; done; rm "$out/$t.svg"; }
alias_to terminal org.kde.konsole utilities-terminal konsole
alias_to files org.kde.dolphin system-file-manager dolphin
alias_to settings systemsettings org.kde.systemsettings preferences-system in.trippp.Settings
alias_to store org.kde.discover plasmadiscover system-software-install
alias_to calendar office-calendar x-office-calendar org.kde.korganizer
alias_to ask in.trippp.Ask
alias_to dictate in.trippp.Dictate
alias_to logo tripppin start-here-kde start-here distributor-logo
