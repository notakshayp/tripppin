#!/bin/sh
# Install Tripppin from the live ISO to a disk (UEFI, GPT, btrfs root).
#
#   doas sh /usr/share/tripppin/install.sh /dev/vda [hostname]
#   (the ISO also ships it as /usr/bin/trip-install)
#
# Layout: p1 = 512M EFI system partition (/boot/efi), p2 = btrfs with subvolumes
#   @           the root filesystem (includes /boot, so kernel and initramfs roll back with it)
#   @snapshots  mounted at /.snapshots, where update snapshots will live
# `trip update` will snapshot @ before `apk upgrade`; rolling back means booting a snapshot.
# ERASES THE WHOLE DISK. A friendly `trip install` will replace this later.
set -eu

disk=${1:?usage: install.sh /dev/DISK [hostname]}
host=${2:-tripppin}
arch=$(uname -m)
repo=${TRIPPPIN_REPO:-/media/cdrom/apks}
[ -d "$repo/$arch" ] || { echo "no package repository at $repo/$arch (are you on the live ISO?)"; exit 1; }
[ -d /sys/firmware/efi ] || { echo "not booted in UEFI mode"; exit 1; }
[ "$(id -u)" = 0 ] || { echo "run as root (doas sh $0 ...)"; exit 1; }

case $arch in
	x86_64) grubtarget=x86_64-efi; extra="mesa-vulkan-intel amd-ucode intel-ucode" ;;
	aarch64) grubtarget=arm64-efi; extra="" ;;
	*) echo "unsupported arch $arch"; exit 1 ;;
esac
fw="tripppin-firmware"

printf 'This ERASES %s. Type the disk name to continue: ' "$disk"
[ "${TRIPPPIN_YES:-}" = "$disk" ] && echo "$disk (from TRIPPPIN_YES)" && ans=$disk || read -r ans
[ "$ans" = "$disk" ] || { echo "aborted"; exit 1; }

part() { case $disk in *[0-9]) echo "${disk}p$1" ;; *) echo "${disk}$1" ;; esac; }
esp=$(part 1); root=$(part 2)

echo "==> partition"
printf 'label: gpt\n,512M,U\n,,L\n' | sfdisk --quiet "$disk"
mdev -s 2>/dev/null || true
sleep 1
echo "==> filesystems"
mkfs.vfat -F32 -n TRIPESP "$esp" >/dev/null
mkfs.btrfs -q -f -L tripppin "$root"
mkdir -p /mnt
mount "$root" /mnt
btrfs subvolume create /mnt/@ >/dev/null
btrfs subvolume create /mnt/@snapshots >/dev/null
umount /mnt
opts=noatime,compress=zstd:1,ssd_spread
mount -o "$opts,subvol=@" "$root" /mnt
mkdir -p /mnt/.snapshots /mnt/boot/efi /mnt/dev /mnt/proc /mnt/sys
mount -o "$opts,subvol=@snapshots" "$root" /mnt/.snapshots
mount "$esp" /mnt/boot/efi
mount --bind /dev /mnt/dev
mount -t proc proc /mnt/proc
mount -t sysfs sys /mnt/sys
mount --bind /sys/firmware/efi/efivars /mnt/sys/firmware/efi/efivars 2>/dev/null || true

echo "==> packages"
mkdir -p /mnt/etc/apk/keys /mnt/etc/mkinitfs
cp /etc/apk/keys/* /mnt/etc/apk/keys/
echo "features=\"ata base ext4 btrfs mmc nvme scsi usb virtio\"" > /mnt/etc/mkinitfs/mkinitfs.conf
apk add --root /mnt --initdb --no-network --keys-dir /etc/apk/keys --repository "$repo" \
	alpine-base alpine-conf linux-lts mkinitfs grub-efi efibootmgr btrfs-progs \
	mesa-vulkan-ati mesa-vulkan-virtio \
	tripppin-base trip llama-cpp-vulkan whisper-cpp $fw $extra
cat > /mnt/etc/apk/repositories <<EOF
https://dl-cdn.alpinelinux.org/alpine/v3.24/main
https://dl-cdn.alpinelinux.org/alpine/v3.24/community
# Tripppin repository (trip update runs apk upgrade against this):
#https://pkgs.trippp.in/v3.24/tripppin
EOF

echo "==> system config"
echo "$host" > /mnt/etc/hostname
ruuid=$(blkid -s UUID -o value "$root"); euuid=$(blkid -s UUID -o value "$esp")
cat > /mnt/etc/fstab <<EOF
UUID=$ruuid / btrfs $opts,subvol=@ 0 0
UUID=$ruuid /.snapshots btrfs $opts,subvol=@snapshots 0 0
UUID=$euuid /boot/efi vfat umask=0077 0 2
tmpfs /tmp tmpfs nosuid,nodev 0 0
EOF
mkdir -p /mnt/etc/default
cat > /mnt/etc/default/grub <<EOF
GRUB_TIMEOUT=1
GRUB_DISABLE_SUBMENU=y
GRUB_DISABLE_RECOVERY=true
GRUB_CMDLINE_LINUX_DEFAULT="quiet loglevel=3 console=tty1"
GRUB_CMDLINE_LINUX="rootfstype=btrfs rootflags=subvol=@ modules=btrfs"
EOF
chroot /mnt sh -c "
mkinitfs \$(ls /lib/modules | head -1) >/dev/null
grub-install --target=$grubtarget --efi-directory=/boot/efi --removable --no-nvram >/dev/null
grub-mkconfig -o /boot/grub/grub.cfg >/dev/null 2>&1
passwd -l root >/dev/null
"
echo "Set the password for the 'trip' user (also used by doas):"
chroot /mnt passwd trip || echo "left at the default password: tripppin (change it with: passwd)"

sync
umount -R /mnt
echo "Done. Remove the ISO and reboot."
