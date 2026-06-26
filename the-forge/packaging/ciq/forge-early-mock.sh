#!/usr/bin/env bash
# Mock-boot only: create runtime dirs without pkill, modprobe, or host /run changes.
set -euo pipefail

mkdir -p \
  /run/dbus /run/forge/log /run/user /run/lock \
  /run/systemd/seats /run/systemd/sessions /run/systemd/users \
  /run/systemd/ask-password /run/systemd/inhibit /run/systemd/machines \
  /run/systemd/shutdown /run/gdm /run/log 2>/dev/null || true

chown root:root /run/dbus 2>/dev/null || true
chmod 0755 /run/dbus 2>/dev/null || true
chown root:gdm /run/gdm 2>/dev/null || true
chmod 0711 /run/gdm 2>/dev/null || true

if getent passwd gdm >/dev/null 2>&1; then
  gdm_uid="$(id -u gdm)"
  mkdir -p "/run/user/${gdm_uid}" 2>/dev/null || true
  chown "gdm:gdm" "/run/user/${gdm_uid}" 2>/dev/null || true
  chmod 0700 "/run/user/${gdm_uid}" 2>/dev/null || true
fi

exit 0