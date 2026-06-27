#!/usr/bin/env bash
# Boot the Sisyphus live USB in QEMU with a graphical display.
set -euo pipefail

DISK="${DISK:-/dev/sda}"
MEM="${MEM:-8G}"
SMP="${SMP:-4}"
SNAPSHOT="${SNAPSHOT:-0}"
OVMF_CODE="${OVMF_CODE:-/usr/share/edk2/ovmf/OVMF_CODE.fd}"
OVMF_VARS_TEMPLATE="${OVMF_VARS_TEMPLATE:-/usr/share/edk2/ovmf/OVMF_VARS.fd}"
OVMF_VARS="${OVMF_VARS:-/tmp/sisyphus-live-usb-ovmf-vars.fd}"
SERIAL_LOG="${SERIAL_LOG:-/tmp/sisyphus-live-usb-serial.log}"

if [[ ! -b "$DISK" ]]; then
    echo "Block device not found: $DISK" >&2
    echo "Set DISK=/dev/sdX to the live USB device." >&2
    exit 1
fi

if [[ ! -r "$OVMF_CODE" || ! -r "$OVMF_VARS_TEMPLATE" ]]; then
    echo "OVMF firmware not found. Install edk2-ovmf." >&2
    exit 1
fi

QEMU_BIN="${QEMU_BIN:-qemu-system-x86_64}"
if ! command -v "$QEMU_BIN" >/dev/null 2>&1; then
    echo "qemu-system-x86_64 not found." >&2
    exit 1
fi

if ! "$QEMU_BIN" -display help 2>&1 | grep -qw gtk; then
    echo "This QEMU build does not support -display gtk." >&2
    exit 1
fi

echo "==> Unmounting ${DISK} partitions (required before raw disk passthrough)"
sudo umount -R "/run/media/${USER}/${DISK##*/}"* 2>/dev/null || true
sudo umount -R /run/media/Sisyphus/disk /run/media/Sisyphus/ROOT 2>/dev/null || true
sudo umount /mnt/sisyphus-efi /mnt/sisyphus-boot 2>/dev/null || true
for part in "${DISK}"*; do
    mountpoint="$(findmnt -n -o TARGET "$part" 2>/dev/null || true)"
    if [[ -n "$mountpoint" ]]; then
        echo "Unmounting $part from $mountpoint"
        sudo umount "$mountpoint"
    fi
done

cp -f "$OVMF_VARS_TEMPLATE" "$OVMF_VARS"

CPU_ARGS=()
if [[ -c /dev/kvm ]] && [[ "${KVM:-1}" == "1" ]]; then
    CPU_ARGS=(-machine q35,accel=kvm -cpu host -smp "$SMP")
else
    CPU_ARGS=(-machine q35 -cpu max -smp "$SMP")
fi

DRIVE_OPTS="file=${DISK},if=none,id=liveusb,format=raw,cache=none"
if [[ "${SNAPSHOT}" == "1" ]]; then
    DRIVE_OPTS="${DRIVE_OPTS},snapshot=on"
    echo "    Snapshot mode: guest writes are discarded"
else
    echo "    Snapshot mode: off (guest writes persist — useful for greeter debugging)"
fi

echo "==> Booting ${DISK} in QEMU (${MEM}, gtk display)"
echo "    Serial log: ${SERIAL_LOG}"
echo "    Close the QEMU window or press Ctrl+C in this terminal to stop."

# Allow root-owned QEMU to open the user graphical session.
if command -v xhost >/dev/null 2>&1 && [[ -n "${DISPLAY:-}" ]]; then
    xhost +local:root >/dev/null 2>&1 || true
fi

QEMU_ENV=(
    "DISPLAY=${DISPLAY:-:0}"
    "XAUTHORITY=${XAUTHORITY:-$HOME/.Xauthority}"
)

exec sudo env "${QEMU_ENV[@]}" "$QEMU_BIN" \
    "${CPU_ARGS[@]}" \
    -m "$MEM" \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$OVMF_VARS" \
    -drive "${DRIVE_OPTS}" \
    -device virtio-blk-pci,drive=liveusb,bootindex=1 \
    -device virtio-net-pci,netdev=net0 \
    -netdev user,id=net0 \
    -device virtio-rng-pci \
    -device qemu-xhci,id=xhci \
    -device usb-kbd,bus=xhci.0 \
    -device usb-tablet,bus=xhci.0 \
    -device virtio-vga \
    -display gtk,show-cursor=on,zoom-to-fit=on \
    -serial "file:${SERIAL_LOG}" \
    -monitor none