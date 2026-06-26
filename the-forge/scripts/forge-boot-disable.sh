#!/usr/bin/env bash
# Restore normal systemd boot — removes init=/usr/sbin/forge-core from all GRUB entries.
set -euo pipefail

FORGE_INIT='init=/usr/sbin/forge-core'

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  echo "forge-boot-disable: run as root (sudo $0)" >&2
  exit 1
fi

echo "Removing forge PID 1 from kernel command line..."

# Restore stock dbus activation for org.freedesktop.systemd1 (real systemd owns the name).
FORGE_DBUS_STOCK="/etc/forge/backup/org.freedesktop.systemd1.service.stock"
FORGE_DBUS_OVERRIDE="/etc/dbus-1/system-services/org.freedesktop.systemd1.service"
FORGE_DBUS_SHARE="/usr/share/dbus-1/system-services/org.freedesktop.systemd1.service"
if [[ -f "$FORGE_DBUS_OVERRIDE" ]] && grep -q "forge/systemd1-stub" "$FORGE_DBUS_OVERRIDE" 2>/dev/null; then
  rm -f "$FORGE_DBUS_OVERRIDE"
  echo "  Removed forge system dbus override for org.freedesktop.systemd1"
fi
if [[ -f "$FORGE_DBUS_SHARE" ]] && grep -q "forge/systemd1-stub" "$FORGE_DBUS_SHARE" 2>/dev/null; then
  if [[ -f "$FORGE_DBUS_STOCK" ]]; then
    cp -a "$FORGE_DBUS_STOCK" "$FORGE_DBUS_SHARE"
    echo "  Restored stock /usr/share system dbus service for org.freedesktop.systemd1"
  else
    rm -f "$FORGE_DBUS_SHARE"
    echo "  Removed forge /usr/share system dbus override for org.freedesktop.systemd1"
  fi
fi
rm -f /etc/NetworkManager/dispatcher.d/99-forge-relabel-resolv.sh 2>/dev/null || true
FORGE_SESSION_DBUS_OVERRIDE="/usr/share/dbus-1/services/org.freedesktop.systemd1.service"
FORGE_SESSION_DBUS_STOCK="/etc/forge/backup/session-org.freedesktop.systemd1.service.stock"
if [[ -f "$FORGE_SESSION_DBUS_OVERRIDE" ]] && grep -q "forge/systemd1-session-stub-wrapper" "$FORGE_SESSION_DBUS_OVERRIDE" 2>/dev/null; then
  if [[ -f "$FORGE_SESSION_DBUS_STOCK" ]]; then
    cp -a "$FORGE_SESSION_DBUS_STOCK" "$FORGE_SESSION_DBUS_OVERRIDE"
    echo "  Restored stock session dbus service for org.freedesktop.systemd1"
  else
    rm -f "$FORGE_SESSION_DBUS_OVERRIDE"
    echo "  Removed forge session dbus override for org.freedesktop.systemd1"
  fi
fi
rm -f /etc/dbus-1/services/org.freedesktop.systemd1.service 2>/dev/null || true

if [[ -f /etc/default/grub ]]; then
  if grep -q "$FORGE_INIT" /etc/default/grub; then
    sed -i "s| ${FORGE_INIT}||g; s|${FORGE_INIT} ||g; s|^${FORGE_INIT}||g" /etc/default/grub
    echo "  Cleared $FORGE_INIT from /etc/default/grub"
  fi
fi

if command -v grubby >/dev/null 2>&1; then
  while read -r idx; do
    [[ -n "$idx" ]] || continue
    grubby --update-kernel="$idx" --remove-args="$FORGE_INIT" 2>/dev/null || true
  done < <(grubby --info=ALL 2>/dev/null | awk -F= '/^kernel=/{print $2}' | sort -u)
  echo "  Removed $FORGE_INIT from grubby kernel entries"
fi

if command -v grub2-mkconfig >/dev/null 2>&1 && [[ -d /boot/grub2 ]]; then
  grub2-mkconfig -o /boot/grub2/grub.cfg >/dev/null 2>&1 || true
fi

# Prefer the stock systemd entry as default when a dedicated forge entry exists.
if command -v grubby >/dev/null 2>&1; then
  default_idx="$(grubby --default-index 2>/dev/null || true)"
  default_title="$(grubby --info=DEFAULT 2>/dev/null | awk -F= '/^title=/{print $2; exit}')"
  if [[ "$default_title" == *"Forge"* ]] || grubby --info=DEFAULT 2>/dev/null | grep -q "$FORGE_INIT"; then
    for idx in $(grubby --info=ALL 2>/dev/null | awk '/^index=/{print $1}' | cut -d= -f2); do
      title="$(grubby --info="$idx" 2>/dev/null | awk -F= '/^title=/{print $2; exit}')"
      args="$(grubby --info="$idx" 2>/dev/null | awk -F= '/^args=/{print $2; exit}')"
      if [[ "$title" != *"Forge"* && "$args" != *"$FORGE_INIT"* ]]; then
        grubby --set-default-index="$idx" 2>/dev/null || true
        echo "  Default boot entry set to: $title"
        break
      fi
    done
  fi
  unset default_idx default_title
fi

echo "Done. Next reboot will use systemd (no init=forge-core unless you pick the Forge menu entry)."