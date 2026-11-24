#!/bin/sh
# Regenerates every raster of the logo from its geometry (crates/trip/src/logo.rs):
# the social images, the app icon, the bar's logo, the wallpapers, the boot splash frames
# and the terminal-logo frames. The SVG sources (brand/*.svg) are written by hand.
#
#   scripts/brand-assets.sh          # on the host: needs cargo and podman
#
# It renders the terminal frames with cargo, then rasterises everything with rsvg-convert in
# a Fedora container (Inter and DM Mono for the social images' type).
set -eu

# The mark, in logo units (the box is 146 x 175 from 27, 12). Keep in step with logo.rs.
ARC='M58.32 70.84 A60 60 0 1 0 141.68 70.84'
ARC_LEN=284.84 # 60 * (360 - 2 * 44) degrees, in radians
SPARK='M100.00 14.00 106.11 43.89 136.00 50.00 106.11 56.11 100.00 86.00 93.89 56.11 64.00 50.00 93.89 43.89Z'
FEDORA=quay.io/fedora/fedora:45
DM_MONO_REV=85f32a0254c400b643d7ce589b91a2ad29bea73b

root=$(cd "$(dirname "$0")/.." && pwd)

if [ "${1:-}" != render ]; then
    mkdir -p "$root/out/brand"
    (cd "$root" && TRIP_LOGO_SVG="$root/out/brand/terminal-logo.svg" \
        cargo test -q -p trip --lib terminal_logo_svg -- --ignored >/dev/null)
    exec podman run --rm -v "$root:/w" -w /w "$FEDORA" sh -c "
        dnf -y -q install --setopt=install_weak_deps=False librsvg2-tools rsms-inter-fonts fontconfig >/dev/null 2>&1 &&
        mkdir -p /usr/share/fonts/dm-mono &&
        curl -fsSL -o /usr/share/fonts/dm-mono/DMMono-Regular.ttf \
          https://raw.githubusercontent.com/google/fonts/$DM_MONO_REV/ofl/dmmono/DMMono-Regular.ttf &&
        fc-cache -f >/dev/null && sh scripts/brand-assets.sh render"
fi

# ---- inside the container ----
cd "$root"
tmp=$(mktemp -d)
calc() { awk "BEGIN { printf \"%.2f\", $1 }"; }

# mark RING_COLOUR SPARK_COLOUR X Y WIDTH [EXTRA_RING_ATTRS]: the mark, its box's top left at X, Y.
mark() {
    s=$(calc "$5 / 146")
    printf '<g transform="translate(%s %s) scale(%s) translate(-27 -12)"><path d="%s" fill="none" stroke="%s" stroke-width="22" stroke-linecap="round"%s/><path d="%s" fill="%s"/></g>' \
        "$3" "$4" "$s" "$ARC" "$1" "${6:-}" "$SPARK" "$2"
}
# centred SIZE_W SIZE_H WIDTH: X and Y that centre the mark's drawn extent (y 14 to 185).
centred() {
    s=$(calc "$3 / 146")
    echo "$(calc "$1 / 2 - 73 * $s") $(calc "$2 / 2 - 87.5 * $s")"
}
svg() { printf '<svg xmlns="http://www.w3.org/2000/svg" width="%s" height="%s">' "$1" "$2"; }
png() { rsvg-convert "$tmp/$1.svg" -o "$2"; echo "  $2"; }

# The dotted grid and green glow behind the social images.
dots() {
    printf '<defs><pattern id="dots" width="26" height="26" patternUnits="userSpaceOnUse"><rect x="12" y="12" width="2" height="2" fill="#0f1413"/></pattern>'
    printf '<radialGradient id="glow" cx="%s" cy="%s" r="%s" gradientUnits="userSpaceOnUse" gradientTransform="%s"><stop offset="0" stop-color="%s"/><stop offset=".5" stop-color="%s"/><stop offset="1" stop-color="#0d1211" stop-opacity="0"/></radialGradient></defs>' "$1" "$2" "$3" "${5:-}" "$4" "$4"
    printf '<rect width="100%%" height="100%%" fill="#0d1211"/><rect width="100%%" height="100%%" fill="url(#dots)"/><rect width="100%%" height="100%%" fill="url(#glow)"/>'
}

echo "brand/social"
for v in "dark #0d1211 #f2f5ef #57d99a" "light #f5f2ea #182423 #1f6b45" "green #1f6b45 #f5f2ea #f5f2ea"; do
    set -- $v
    { svg 1024 1024; printf '<rect width="1024" height="1024" fill="%s"/>' "$2"; mark "$3" "$4" $(centred 1024 1024 466) 466; echo '</svg>'; } > "$tmp/avatar-$1.svg"
    png "avatar-$1" "brand/social/tripppin-avatar-$1.png"
done
{ svg 1000 1000; dots 500 500 430 '#174730'; mark '#f2f5ef' '#57d99a' $(centred 1000 1000 450) 450; echo '</svg>'; } > "$tmp/x-avatar.svg"
png x-avatar brand/social/tripppin-x-avatar.png
{
    svg 1500 500
    dots 1125 90 340 '#16432d' 'translate(1125 90) scale(1 1.15) translate(-1125 -90)'
    mark '#f2f5ef' '#57d99a' 469.4 137.4 191
    printf '<text x="702" y="240" font-family="Inter" font-weight="800" font-size="129" letter-spacing="-5" fill="#f2f5ef">tripppin</text>'
    printf '<text x="706" y="308" font-family="Inter" font-weight="600" font-size="34" letter-spacing="-0.8" fill="#f2f5ef">tell your computer what you want. <tspan fill="#57d99a">it does it.</tspan></text>'
    printf '<text x="707" y="360" font-family="DM Mono" font-size="19" letter-spacing="1" fill="#9aaba3">a linux with an ai inside · trippp.in</text>'
    echo '</svg>'
} > "$tmp/x-header.svg"
png x-header brand/social/tripppin-x-header.png
{
    svg 1080 1080
    dots 480 380 500 '#153a28'
    printf '<text x="85" y="105" font-family="DM Mono" font-size="21" letter-spacing="1" fill="#9aaba3">a linux with an ai inside</text>'
    mark '#f2f5ef' '#57d99a' 84.2 245 218
    printf '<text x="80" y="668" font-family="Inter" font-weight="800" font-size="148" letter-spacing="-6" fill="#f2f5ef">tripppin</text>'
    printf '<text font-family="Inter" font-weight="600" font-size="51" letter-spacing="-1.5" fill="#f2f5ef"><tspan x="84" y="768">tell your computer</tspan><tspan x="84" y="825">what you want. <tspan fill="#57d99a">it does it.</tspan></tspan></text>'
    printf '<text x="85" y="990" font-family="DM Mono" font-size="22" letter-spacing="1" fill="#9aaba3">trippp.in</text>'
    printf '<text x="995" y="990" text-anchor="end" font-family="DM Mono" font-size="22" letter-spacing="1" fill="#9aaba3">coming soon</text>'
    echo '</svg>'
} > "$tmp/x-post.svg"
png x-post brand/social/tripppin-x-post.png

echo "brand/terminal-logo.png"
cp out/brand/terminal-logo.svg "$tmp/terminal-logo.svg"
png terminal-logo brand/terminal-logo.png

echo "os/files"
{ svg 256 256; printf '<rect width="256" height="256" fill="#0d1211"/>'; mark '#f2f5ef' '#57d99a' $(centred 256 256 117) 117; echo '</svg>'; } > "$tmp/icon.svg"
png icon os/files/usr/share/icons/hicolor/256x256/apps/tripppin.png
{ svg 30 36; mark '#f2f5ef' '#57d99a' 0 0 30; echo '</svg>'; } > "$tmp/bar.svg"
png bar os/files/usr/share/tripppin/logo-bar.png
# The wallpapers: the huge, faint mark off the top right (common::backdrop draws the same).
for v in "ink #0b100f #111816 #11271d" "night #05080a #0c1214 #0f2a1e" "forest #123526 #18442f #1f6b45" "paper #f2efe6 #e4dfd2 #cfe0d2"; do
    set -- $v
    { svg 2560 1600; printf '<rect width="2560" height="1600" fill="%s"/>' "$2"; mark "$3" "$4" 1280 -300 1800; echo '</svg>'; } > "$tmp/wall-$1.svg"
    png "wall-$1" "os/files/usr/share/tripppin/wallpapers/$1.png"
done
cp os/files/usr/share/tripppin/wallpapers/ink.png os/files/usr/share/tripppin/wallpaper.png

# The boot splash (tripppin.script): the ring drawing itself in 24 frames, the finished ring,
# and the spark alone on a square, centred, so Plymouth can spin it about its middle. 2x.
theme=os/files/usr/share/plymouth/themes/tripppin
rm -f "$theme"/ring-*.png
i=1
while [ $i -le 24 ]; do
    dash=$(calc "$ARC_LEN * $i / 24")
    { svg 292 350; mark '#f2f5ef' none 0 0 292 " stroke-dasharray=\"$dash 1000\""; echo '</svg>'; } > "$tmp/ring-$i.svg"
    rsvg-convert "$tmp/ring-$i.svg" -o "$theme/ring-$i.png"
    i=$((i + 1))
done
echo "  $theme/ring-1..24.png"
{ svg 292 350; mark '#f2f5ef' none 0 0 292; echo '</svg>'; } > "$tmp/ring.svg"
png ring "$theme/ring.png"
{ svg 192 192; printf '<path transform="translate(96 96) scale(2) translate(-100 -50)" d="%s" fill="#57d99a"/>' "$SPARK"; echo '</svg>'; } > "$tmp/spark.svg"
png spark "$theme/spark.png"
rm -rf "$tmp"
