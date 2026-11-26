#!/bin/sh
# Build the Tripppin apk packages (tripppin-base, trip, llama-cpp-vulkan, whisper-cpp)
# inside the pinned Alpine builder container.
#
#   alpine/build-packages.sh [aarch64|x86_64] [pkg ...]
#
# Output: alpine/out/repo/<arch>/ (signed index + apks), alpine/out/keys/ (signing key).
# Environment:
#   PODMAN_CONNECTION  podman connection to use (default: tripppin-build-root)
#   ABUILD_PRIVKEY     path to a private key to sign with (default: a local throwaway key
#                      under alpine/out/keys; CI supplies the real one from a secret)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
arch=${1:-$(uname -m | sed 's/arm64/aarch64/')}
[ $# -gt 0 ] && shift
pkgs=${*:-"tripppin-base trip llama-cpp-vulkan whisper-cpp"}
conn=${PODMAN_CONNECTION:-tripppin-build-root}
pm="podman --connection $conn"
case $arch in
	aarch64) platform=linux/arm64 ;;
	x86_64) platform=linux/amd64 ;;
	*) echo "unknown arch $arch" >&2; exit 1 ;;
esac

out=$here/out
mkdir -p "$out/src" "$out/keys" "$out/repo"
ver=$(sed -n 's/^pkgver=//p' "$here/packages/trip/APKBUILD")
git -C "$root" archive --format=tar.gz --prefix="tripppin-$ver/" -o "$out/src/tripppin-$ver.tar.gz" HEAD

$pm build -q --platform "$platform" -f "$here/build/Containerfile" -t "tripppin-alpine-builder:$arch" "$here/build" >/dev/null

# The remote podman machine sees its own filesystem, so stream inputs in via a volume.
vol=tripppin-pkg-$arch
$pm volume exists "$vol" || $pm volume create "$vol" >/dev/null
$pm volume exists tripppin-cargo || $pm volume create tripppin-cargo >/dev/null
tar --no-xattrs -C "$here" -cf - packages | $pm run --rm -i --platform "$platform" -v "$vol":/work "tripppin-alpine-builder:$arch" \
	sh -c 'rm -rf /work/packages && tar -C /work -xf - && chown -R builder:abuild /work'
$pm run --rm -i --platform "$platform" -v "$vol":/work "tripppin-alpine-builder:$arch" \
	sh -c 'cat > /work/packages/trip/tripppin-'"$ver"'.tar.gz && chown builder:abuild /work/packages/trip/tripppin-'"$ver"'.tar.gz' \
	< "$out/src/tripppin-$ver.tar.gz"
if [ -n "${ABUILD_PRIVKEY:-}" ]; then
	$pm run --rm -i --platform "$platform" -v "$vol":/work "tripppin-alpine-builder:$arch" \
		sh -c 'mkdir -p /work/keys && cat > /work/keys/tripppin.rsa && chown -R builder:abuild /work/keys && chmod 600 /work/keys/tripppin.rsa' \
		< "$ABUILD_PRIVKEY"
fi

$pm run --rm --platform "$platform" -v "$vol":/work -v tripppin-cargo:/home/builder/.cargo -e PKGS="$pkgs" \
	"tripppin-alpine-builder:$arch" su builder -c '
set -eu
cd /work
mkdir -p keys repo
if [ ! -f keys/tripppin.rsa ]; then
	openssl genrsa -out keys/tripppin.rsa 4096 2>/dev/null
	openssl rsa -in keys/tripppin.rsa -pubout -out keys/tripppin.rsa.pub 2>/dev/null
fi
[ -f keys/tripppin.rsa.pub ] || openssl rsa -in keys/tripppin.rsa -pubout -out keys/tripppin.rsa.pub 2>/dev/null
mkdir -p ~/.abuild
echo "PACKAGER_PRIVKEY=/work/keys/tripppin.rsa" > ~/.abuild/abuild.conf
echo "PACKAGER=\"Tripppin <hello@trippp.in>\"" >> ~/.abuild/abuild.conf
sudo cp keys/tripppin.rsa.pub /etc/apk/keys/tripppin.rsa.pub
sudo apk update -q
export REPODEST=/work/repo
mkdir -p /work/aport
for p in $PKGS; do
	rm -rf /work/aport/tripppin/$p
	mkdir -p /work/aport/tripppin
	cp -r /work/packages/$p /work/aport/tripppin/$p
	cd /work/aport/tripppin/$p
	abuild checksum
	flags=""; [ $p = tripppin-base ] && flags=-d
	abuild $flags -r -P /work/repo
	cd /work
done
ls /work/repo/tripppin/*/*.apk
'

# Copy the finished repository and public key out of the volume.
rm -rf "$out/repo/$arch" && mkdir -p "$out/repo/$arch"
$pm run --rm --platform "$platform" -v "$vol":/work "tripppin-alpine-builder:$arch" \
	sh -c 'cd /work/repo/tripppin/'"$arch"' && tar -cf - .' | tar -C "$out/repo/$arch" -xf -
$pm run --rm --platform "$platform" -v "$vol":/work "tripppin-alpine-builder:$arch" \
	cat /work/keys/tripppin.rsa.pub > "$out/keys/tripppin.rsa.pub"
echo "repository: $out/repo/$arch  (key: $out/keys/tripppin.rsa.pub)"
