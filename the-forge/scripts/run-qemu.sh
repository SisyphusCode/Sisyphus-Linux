#!/usr/bin/env bash
# Build initramfs (or full system clone) and boot it in QEMU with forge as init.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KERNEL="${KERNEL:-$(ls -1 /boot/vmlinuz-* 2>/dev/null | sort -V | tail -1 || true)}"

# FULL=1 or CLONE=full  => use dracut-based full CIQ RLC Pro AI clone (exact packages + root FS snapshot)
#   This is the "full CIQ RLC Pro AI system clone for QEMU" requested.
#   It uses the host's dracut logic + injects current forge + /etc/forge + scripts,
#   then presents the real root (via safe snapshot) to forge-core.
# Otherwise (default): use the smaller synthetic forge-initramfs.cpio.gz (quick early-boot tests)
if [[ "${FULL:-}" == "1" || "${CLONE:-}" == "full" ]]; then
  MODE=full
  INITRD="${INITRD:-$ROOT/target/forge-full-clone-initrd.img}"
  echo "=== FULL CIQ system clone mode ==="
  echo "Using host dracut + virtio + injected forge for faithful root FS mock."
  "$ROOT/scripts/build-full-clone-initrd.sh"
else
  MODE=small
  INITRD="${INITRD:-$ROOT/target/forge-initramfs.cpio.gz}"
  "$ROOT/scripts/build-initramfs.sh"
fi

if [[ -z "$KERNEL" || ! -f "$KERNEL" ]]; then
  echo "Set KERNEL=/path/to/vmlinuz to launch QEMU automatically." >&2
  exit 0
fi

QEMU_BIN=""
if command -v qemu-system-x86_64 >/dev/null 2>&1; then
  QEMU_BIN=$(command -v qemu-system-x86_64)
elif [ -x /usr/libexec/qemu-kvm ]; then
  QEMU_BIN=/usr/libexec/qemu-kvm
else
  echo "No qemu-system-x86_64 or /usr/libexec/qemu-kvm found; initrd built at $INITRD" >&2
  exit 0
fi
echo "Using QEMU: $QEMU_BIN"

echo "Booting kernel $KERNEL with $MODE initrd ($INITRD) ..."

CPU_FLAG=""
if [[ "$QEMU_BIN" == *qemu-kvm* ]]; then
  CPU_FLAG="-cpu host -enable-kvm"
fi

# Detect if this QEMU supports gtk display (many qemu-kvm packages on servers do not).
# Fallback to nographic + serial (best for seeing boot waves, GDM/forge logs anyway).
if "$QEMU_BIN" -display help 2>&1 | grep -qw gtk; then
  DISPLAY_OPTS="-vga std -serial mon:stdio -display gtk"
else
  echo "Note: $QEMU_BIN does not support gtk display (typical for /usr/libexec/qemu-kvm). Falling back to serial console."
  DISPLAY_OPTS="-nographic -serial mon:stdio -display none"
fi

# Common devices
NET_ARGS="-netdev user,id=net0 -device virtio-net-pci,netdev=net0 -device virtio-rng-pci"

if [[ "$MODE" == "full" ]]; then
  # === Full faithful clone: present the real CIQ root (LVM content) read/write via qemu COW snapshot ===
  # -drive on the mapper gives the full XFS (GDM, gnome-shell, NM, plymouth, nvidia libs, all /etc, /usr ...)
  # We tell dracut (still running in the injected initrd) to mount it as vda and exec our forge.
  # snapshot=on => guest writes are thrown away, host stays untouched. Safe.
  # Requires root for opening /dev/mapper (script uses sudo for the qemu launch in this mode).

  ROOT_DISK="/dev/mapper/Sisyphus-root"
  if [[ ! -e "$ROOT_DISK" ]]; then
    echo "ERROR: $ROOT_DISK not found. Cannot do full clone without the LVM LV." >&2
    exit 1
  fi

  # Adapted cmdline:
  #  - root=/dev/vda (the virtio-blk device QEMU exposes for if=virtio drive)
  #  - init=/usr/bin/forge-core  (copied by our pre-pivot hook)
  #  - Keep useful flags, drop host-specific rd.lvm. that would be confused (we already have the content)
  #  - Enable serial console + enough logging to see waves + failures
  APPEND="root=/dev/vda rw console=ttyS0,115200 console=tty0 init=/usr/bin/forge-core loglevel=6 rd.info rd.debug debug"

  MEM="${MEM:-6G}"
  SMP="${SMP:-4}"

  if [[ "${DEBUG:-}" == "1" || "${CONSOLE:-}" == "1" ]]; then
    echo "FULL-CLONE DEBUG: serial only (nographic). This shows full waves, service launches, GDM/NM/plymouth errors from the real FS."
    # Use sudo to access host block device for the snapshot drive.
    exec sudo "$QEMU_BIN" $CPU_FLAG \
      -m "$MEM" \
      -smp "$SMP" \
      -kernel "$KERNEL" \
      -initrd "$INITRD" \
      -append "$APPEND" \
      -drive file="$ROOT_DISK",format=raw,if=virtio,snapshot=on \
      $NET_ARGS \
      $DISPLAY_OPTS 2>&1 | tee -a /tmp/forge-qemu-full-serial.log
  else
    echo "FULL-CLONE: graphical attempt (vga). Use DEBUG=1 for serial diagnosis of hangs/failures."
    exec sudo "$QEMU_BIN" $CPU_FLAG \
      -m "$MEM" \
      -smp "$SMP" \
      -kernel "$KERNEL" \
      -initrd "$INITRD" \
      -append "$APPEND" \
      -drive file="$ROOT_DISK",format=raw,if=virtio,snapshot=on \
      $NET_ARGS \
      $DISPLAY_OPTS 2>&1 | cat
  fi
else
  # === Original smaller synthetic initramfs (quick iteration for early waves / fd activation etc) ===
  if [[ "${DEBUG:-}" == "1" || "${CONSOLE:-}" == "1" ]]; then
    echo "Running in DEBUG/CONSOLE mode (serial output, no graphics display)..."
    exec "$QEMU_BIN" $CPU_FLAG \
      -m 2G \
      -smp 2 \
      -kernel "$KERNEL" \
      -initrd "$INITRD" \
      -append "console=ttyS0,115200 console=tty0 init=/sbin/init rdinit=/sbin/init debug" \
      $NET_ARGS \
      -nographic \
      -serial mon:stdio \
      -display none 2>&1 | tee -a /tmp/forge-qemu-serial.log
  else
    exec "$QEMU_BIN" $CPU_FLAG \
      -m 2G \
      -smp 2 \
      -kernel "$KERNEL" \
      -initrd "$INITRD" \
      -append "console=ttyS0,115200 console=tty0 init=/sbin/init rdinit=/sbin/init" \
      -netdev user,id=net0,hostfwd=tcp::2222-:22 \
      -device virtio-net-pci,netdev=net0 \
      -device virtio-rng-pci \
      $DISPLAY_OPTS 2>&1 | cat
  fi
fi

# Notes:
# - For the *full* faithful diagnosis run:  DEBUG=1 FULL=1 ./scripts/run-qemu.sh
# - Logs: /tmp/forge-qemu-full-serial.log (or the small one)
# - The full clone gives you the exact CIQ packages, /etc/forge units, start-*.sh scripts,
#   gnome/mutter/NM/plymouth from the real root, so failures match the "alot of failures"
#   seen on bare metal.
# - If virtio disk not detected: check dmesg in serial for "VFS: Unable to mount", then
#   we may need extra --add-drivers or different if=scsi.
# - After a run you can inspect the serial log for ⏱️ , LAUNCHING, FAILED, dbus errors etc.


# Notes:
# - For graphical GDM test in QEMU: use non-DEBUG, but ensure initramfs has gdm, Xorg, drivers, fonts (see build-initramfs comments).
# - To add a root disk for more complete test: add -drive file=your-root.img,if=virtio
# - To debug hang: use DEBUG=1, then in serial you see WAVE logs, dbus start attempts, etc.
# - Logs from forge inside: look for /var/log/forge/* in the guest (or add -serial to file).
# - If qemu not found, the script builds the initrd and tells you to install qemu-kvm or qemu-system-x86.
