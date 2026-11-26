#!/bin/sh -e
# Builds the apkovl (initial /etc) for the live ISO. Modelled on aports genapkovl-dhcp.sh.

HOSTNAME="$1"
[ -n "$HOSTNAME" ] || { echo "usage: $0 hostname"; exit 1; }

cleanup() { rm -rf "$tmp"; }
tmp="$(mktemp -d)"
trap cleanup EXIT

rc_add() {
	mkdir -p "$tmp"/etc/runlevels/"$2"
	ln -sf /etc/init.d/"$1" "$tmp"/etc/runlevels/"$2"/"$1"
}

mkdir -p "$tmp"/etc/apk
echo "$HOSTNAME" > "$tmp"/etc/hostname

# Everything the image needs is on the ISO; trip update later adds the network repository.
cat > "$tmp"/etc/apk/world <<EOF
alpine-base
alpine-conf
tripppin-base
trip
llama-cpp-vulkan
whisper-cpp
mkinitfs
btrfs-progs
dosfstools
efibootmgr
grub-efi
sfdisk
mesa-vulkan-ati
mesa-vulkan-virtio
vulkan-tools
EOF

mkdir -p "$tmp"/etc/iwd
cat > "$tmp"/etc/iwd/main.conf <<'EOF'
[General]
EnableNetworkConfiguration=false
[Network]
NameResolvingService=resolvconf
EOF

# Console: autologin the primary user on tty1 and run the compositor from its profile.
cat > "$tmp"/etc/inittab <<'EOF'
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

tty1::respawn:/sbin/getty -n -l /usr/libexec/tripppin/autologin 38400 tty1 # tripppin
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOF

rc_add devfs sysinit
rc_add dmesg sysinit
rc_add udev sysinit
rc_add udev-trigger sysinit
rc_add modloop sysinit

rc_add modules boot
rc_add sysctl boot
rc_add hostname boot
rc_add bootmisc boot
rc_add syslog boot

for s in trip-runtime seatd dbus iwd bluetooth dhcpcd acpid chronyd; do rc_add $s default; done

rc_add mount-ro shutdown
rc_add killprocs shutdown
rc_add savecache shutdown

tar -c -C "$tmp" etc | gzip -9n > "$HOSTNAME.apkovl.tar.gz"
