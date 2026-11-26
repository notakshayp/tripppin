#!/bin/sh
# Boot the Tripppin ISO (or an installed disk) in QEMU on an Apple Silicon Mac, with HVF.
#
#   alpine/qemu.sh iso            boot the live ISO (alpine/out/iso/aarch64/*.iso)
#   alpine/qemu.sh disk           boot alpine/out/disk.qcow2 (after installing; see README)
#   alpine/qemu.sh iso+disk       ISO plus the blank/installed disk (to run install.sh)
#
# VNC is on :1 (port 5901); the monitor is a unix socket at $MON, so screenshots work with:
#   echo "screendump /path/shot.ppm" | nc -U $MON
# Serial console goes to $SERIAL (log file) and port 4555 over telnet-less TCP.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
mode=${1:-iso}
out=$here/out
share=$(dirname "$(command -v qemu-system-aarch64)")/../share/qemu
mon=${MON:-$out/qemu-mon.sock}
serial=${SERIAL:-$out/serial.log}
iso=$(ls "$out"/iso/aarch64/*.iso | head -1)
[ -f "$out/efivars.fd" ] || dd if=/dev/zero of="$out/efivars.fd" bs=1m count=64 2>/dev/null
[ -f "$out/disk.qcow2" ] || qemu-img create -q -f qcow2 "$out/disk.qcow2" 16G

args="-M virt,highmem=on -accel hvf -cpu host -smp 4 -m 4096
 -drive if=pflash,format=raw,readonly=on,file=$share/edk2-aarch64-code.fd
 -drive if=pflash,format=raw,file=$out/efivars.fd
 -device virtio-gpu-pci -device qemu-xhci -device usb-kbd -device usb-tablet
 -netdev user,id=n0,hostfwd=tcp::2222-:22 -device virtio-net-pci,netdev=n0
 -display none -vnc :1
 -monitor unix:$mon,server,nowait
 -serial file:$serial"
case $mode in
	iso) args="$args -drive if=none,id=cd,media=cdrom,readonly=on,file=$iso -device usb-storage,drive=cd,bootindex=1" ;;
	disk) args="$args -drive if=none,id=hd,file=$out/disk.qcow2 -device virtio-blk-pci,drive=hd,bootindex=1" ;;
	iso+disk) args="$args -drive if=none,id=cd,media=cdrom,readonly=on,file=$iso -device usb-storage,drive=cd,bootindex=1
 -drive if=none,id=hd,file=$out/disk.qcow2 -device virtio-blk-pci,drive=hd,bootindex=2" ;;
esac
# shellcheck disable=SC2086
exec qemu-system-aarch64 $args
