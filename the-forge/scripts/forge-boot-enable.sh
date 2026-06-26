#!/usr/bin/env bash
# Enable forge as PID 1 via kernel cmdline args on all installed kernels (grubby).
set -euo pipefail

FORGE_NATIVE="${FORGE_NATIVE_MODE:-1}"
FORGE_INIT='init=/usr/sbin/forge-core'
if [[ "$FORGE_NATIVE" == "1" || "$FORGE_NATIVE" == "true" ]]; then
  FORGE_INIT="$FORGE_INIT FORGE_NATIVE_MODE=1"
fi

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  echo "forge-boot-enable: run as root (sudo $0)" >&2
  exit 1
fi

if ! command -v grubby >/dev/null 2>&1; then
  echo "forge-boot-enable: grubby not found (Rocky/RHEL required)" >&2
  exit 1
fi

echo "Enabling forge PID 1 on all kernels: $FORGE_INIT"

grubby --update-kernel=ALL --args="$FORGE_INIT"

if command -v grub2-mkconfig >/dev/null 2>&1 && [[ -d /boot/grub2 ]]; then
  grub2-mkconfig -o /boot/grub2/grub.cfg >/dev/null 2>&1 || true
fi

if grubby --info=DEFAULT 2>/dev/null | grep -qF "$FORGE_INIT" || \
   grep -rlF "$FORGE_INIT" /boot/loader/entries/ >/dev/null 2>&1; then
  echo "Verified: $FORGE_INIT is on the default boot entry."
else
  echo "WARN: grubby ran but $FORGE_INIT not visible — check: sudo grubby --info=DEFAULT" >&2
fi

echo "Done. Reboot to boot with forge-core as PID 1."
echo "Recovery stays on Forge by default. Set FORGE_RECOVERY_HANDOFF=1 in cmdline for systemd fallback."
echo "To revert: sudo forge-boot-disable"