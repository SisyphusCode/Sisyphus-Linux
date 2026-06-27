#!/bin/bash
set -euo pipefail
DISK="${DISK:-/dev/sda}"
MEM="${MEM:-8G}"
SMP="${SMP:-4}"
TIMEOUT="${TIMEOUT:-0}"
OVMF_CODE=/usr/share/edk2/ovmf/OVMF_CODE.fd
OVMF_VARS=/tmp/sisyphus-ovmf-vars.fd
SERIAL_LOG=/tmp/sisyphus-serial.log
MONITOR_SOCK=/tmp/qemu-monitor.sock
PID_FILE=/tmp/sisyphus-qemu.pid

cp -f /usr/share/edk2/ovmf/OVMF_VARS.fd "$OVMF_VARS" || true

echo "==> Unmounting ${DISK} partitions"
sudo umount -R /run/media/Sisyphus/disk /run/media/Sisyphus/ROOT 2>/dev/null || true
for p in "${DISK}"*; do sudo umount "$p" 2>/dev/null || true; done

sudo rm -f "$SERIAL_LOG" "$MONITOR_SOCK" "$PID_FILE" 2>/dev/null || true

echo "==> Launching QEMU with virtio-gpu-gl for COSMIC greeter test"
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
  -pidfile "$PID_FILE" \
  -daemonize || { echo "QEMU launch failed"; exit 1; }

sleep 5
if [ ! -S "$MONITOR_SOCK" ]; then
  echo "No monitor socket"
  exit 1
fi

if [ ! -s "$PID_FILE" ]; then
  echo "QEMU pidfile not created"
  exit 1
fi

QEMU_PID="$(cat "$PID_FILE")"
echo "==> QEMU running (pid=${QEMU_PID})."
echo "    Serial log: ${SERIAL_LOG}"

if [ "$TIMEOUT" -gt 0 ] 2>/dev/null; then
  echo "==> Waiting ${TIMEOUT}s before requesting shutdown"
  sleep "$TIMEOUT"
else
  echo "==> Press Enter when finished to request guest shutdown"
  read -r _
fi

echo "system_powerdown" | sudo nc -U -w 3 "$MONITOR_SOCK" || true
for _ in $(seq 1 10); do
  if ! sudo kill -0 "$QEMU_PID" 2>/dev/null; then
    echo "==> QEMU exited cleanly."
    exit 0
  fi
  sleep 1
done

echo "==> QEMU still running, stopping pid ${QEMU_PID}"
sudo kill "$QEMU_PID"
