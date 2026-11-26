# aports mkimage profile for the Tripppin live/installer ISO.
# Installed to ~/.mkimage/ in the builder container; run with `--profile tripppin`.
#
# Base set only. The kernel is linux-lts, firmware is a subset of linux-firmware, and the
# model files are not baked in (trip downloads them on first boot).

# Firmware subset lives in the tripppin-firmware meta package (packages/tripppin-firmware).
tripppin_firmware="tripppin-firmware"

# Same subset for the kernel (modloop) section, replacing the stock "linux-firmware".
section_kernels() {
	local _f _pkgs
	for _f in $kernel_flavors; do
		_pkgs="$tripppin_firmware linux-$_f"
		local id=$( (echo "$initfs_features::$_hostkeys" ; apk fetch --root "$APKROOT" --simulate alpine-base $_pkgs | sort) | checksum)
		build_section kernel $ARCH $_f $id $_pkgs
	done
}

profile_tripppin() {
	profile_base
	profile_abbrev="trip"
	title="Tripppin"
	desc="Tell your computer what you want. It does it."
	image_ext="iso"
	arch="aarch64 x86_64"
	output_format="iso"
	hostname="tripppin"
	apkovl="genapkovl-tripppin.sh"
	kernel_flavors="lts"
	kernel_addons=
	initfs_features="ata base cdrom ext4 btrfs mmc nvme scsi squashfs usb virtio"
	initfs_cmdline="modules=loop,squashfs,sd-mod,usb-storage,btrfs quiet"
	kernel_cmdline="loglevel=3 console=tty1"

	apks="alpine-base alpine-conf busybox openrc mkinitfs
		e2fsprogs btrfs-progs dosfstools sfdisk util-linux-misc efibootmgr grub-efi
		linux-lts $tripppin_firmware
		mesa-vulkan-ati mesa-vulkan-virtio vulkan-tools
		tripppin-base trip llama-cpp-vulkan whisper-cpp"
	case "$ARCH" in
	x86_64)
		apks="$apks mesa-vulkan-intel amd-ucode intel-ucode"
		boot_addons="amd-ucode intel-ucode"
		initrd_ucode="/boot/amd-ucode.img /boot/intel-ucode.img"
		;;
	aarch64)
		kernel_cmdline="$kernel_cmdline console=ttyAMA0"
		;;
	esac
}
