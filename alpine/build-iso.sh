#!/bin/sh
# Build the Tripppin live/installer ISO with Alpine's own mkimage, inside the builder container.
#
#   alpine/build-iso.sh [aarch64|x86_64]
#
# Needs alpine/out/repo/<arch> from build-packages.sh. Output: alpine/out/iso/<arch>/*.iso
# (hybrid UEFI ISO; x86_64 also boots BIOS) plus checksums.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
arch=${1:-$(uname -m | sed 's/arm64/aarch64/')}
conn=${PODMAN_CONNECTION:-tripppin-build-root}
pm="podman --connection $conn"
alpine=v3.24
tag=v3.24.4
case $arch in
	aarch64) platform=linux/arm64 ;;
	x86_64) platform=linux/amd64 ;;
	*) echo "unknown arch $arch" >&2; exit 1 ;;
esac
[ -f "$here/out/repo/$arch/APKINDEX.tar.gz" ] || { echo "run build-packages.sh $arch first" >&2; exit 1; }

img=tripppin-alpine-builder:$arch
$pm build -q --platform "$platform" -f "$here/build/Containerfile" -t "$img" "$here/build" >/dev/null
vol=tripppin-iso-$arch
$pm volume exists "$vol" || $pm volume create "$vol" >/dev/null

# Stage the signed repository, public key and profile into the volume.
tar --no-xattrs -C "$here/out" -cf - repo/$arch keys/tripppin.rsa.pub -C "$here" mkimage | \
	$pm run --rm -i --platform "$platform" -v "$vol":/work "$img" \
	sh -c 'rm -rf /work/repo /work/mkimage /work/iso /work/tmp && tar -C /work -xf - && mkdir -p /work/repo-root/tripppin && rm -rf /work/repo-root/tripppin/'"$arch"' && mv /work/repo/'"$arch"' /work/repo-root/tripppin/'"$arch"' && chown -R builder:abuild /work'
if [ -n "${ABUILD_PRIVKEY:-}" ]; then
	$pm run --rm -i --platform "$platform" -v "$vol":/work "$img" \
		sh -c 'cat > /work/keys/tripppin.rsa && chown builder:abuild /work/keys/tripppin.rsa && chmod 600 /work/keys/tripppin.rsa' < "$ABUILD_PRIVKEY"
else
	# Reuse the throwaway key from the package build (same volume layout as build-packages.sh).
	$pm run --rm --platform "$platform" -v tripppin-pkg-$arch:/pkg -v "$vol":/work "$img" \
		sh -c 'cp /pkg/keys/tripppin.rsa /work/keys/tripppin.rsa && chown builder:abuild /work/keys/tripppin.rsa && chmod 600 /work/keys/tripppin.rsa'
fi

$pm run --rm --platform "$platform" --privileged -v "$vol":/work "$img" su builder -c "
set -eu
mkdir -p ~/.abuild ~/.mkimage
printf 'PACKAGER_PRIVKEY=/work/keys/tripppin.rsa\nPACKAGER=\"Tripppin <hello@trippp.in>\"\n' > ~/.abuild/abuild.conf
cp /work/mkimage/* ~/.mkimage/
sudo cp /work/keys/tripppin.rsa.pub /etc/apk/keys/
sudo apk update -q
# update-kernel always adds the whole linux-firmware to the modloop; use only our subset.
sudo sed -i 's/PACKAGES=\"\$PACKAGES linux-\$FLAVOR linux-firmware\"/PACKAGES=\"\$PACKAGES linux-\$FLAVOR\"/' /usr/sbin/update-kernel
grep -q 'linux-firmware\"' /usr/sbin/update-kernel && { echo 'update-kernel patch failed' >&2; exit 1; }
cd ~/aports/scripts
mkdir -p /work/iso /work/tmp
sh mkimage.sh --tag $tag --outdir /work/iso --workdir /work/tmp --arch $arch --hostkeys \
	--repository https://dl-cdn.alpinelinux.org/alpine/$alpine/main \
	--repository https://dl-cdn.alpinelinux.org/alpine/$alpine/community \
	--repository /work/repo-root/tripppin \
	--profile tripppin
ls -l /work/iso
"

rm -rf "$here/out/iso/$arch" && mkdir -p "$here/out/iso/$arch"
$pm run --rm --platform "$platform" -v "$vol":/work "$img" sh -c 'cd /work/iso && tar -cf - .' | tar -C "$here/out/iso/$arch" -xf -
ls -lh "$here/out/iso/$arch"
