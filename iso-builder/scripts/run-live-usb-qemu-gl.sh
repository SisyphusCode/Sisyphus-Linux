#!/bin/bash
set -euo pipefail
DISK="${DISK:-/dev/sda}"
MEM="${MEM:-8G}"
SMP="${SMP:-4}"
OVMF_CODE=/usr/share/edk2/ovmf/OVMF_CODE.fd
OVMF_VARS=/tmp/sisyphus-ovmf-vars.fd
SERIAL_LOG=/tmp/sisyphus-serial.log
MONITOR_SOCK=/tmp/qemu-monitor.sock

cp -f /usr/share/edk2/ovmf/OVMF_VARS.fd "$OVMF_VARS" || true

echo "==> Unmounting ${DISK} partitions"
sudo umount -R /run/media/Sisyphus/disk /run/media/Sisyphus/ROOT 2>/dev/null || true
for p in "${DISK}"*; do sudo umount "$p" 2>/dev/null || true; done

sudo rm -f "$SERIAL_LOG" "$MONITOR_SOCK" 2>/dev/null || true

echo "==> Launching QEMU with virtio-gpu-gl for COSMIC greeter test (will run 70s)"
sudo qemu-system-x86_64 \
  -machine q35,accel=kvm -cpu host -smp "$SMP" -m "$MEM" \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS" \
  -drive file="$DISK",if=none,id=liveusb,format=raw,cache=none \
  -device virtio-blk-pci,drive=liveusb,bootindex=1 \
  -device virtio-gpu-gl-pci \
  -display gtk,gl=on,show-cursor=on,grab-on-hover=on \
  -device qemu-xhci,id=xhci \
  -device usb-tablet,bus=xhci.0 \
  -serial "file:$SERIAL_LOG" \
  -monitor "unix:$MONITOR_SOCK,server,nowait" \
  -daemonize || { echo "QEMU launch failed"; exit 1; }

sleep 5
if [ ! -S "$MONITOR_SOCK" ]; then
  echo "No monitor socket"
  exit 1
fi

echo "==> Waiting 65s for greeter GUI to appear in QEMU window..."
sleep 65

echo "system_powerdown" | sudo nc -U -w 3 "$MONITOR_SOCK" || true
sleep 5
sudo pkill -f "qemu.*$DISK" || true
echo "==> QEMU stopped. Check logs on USB for success (no panic, backend with output)."
