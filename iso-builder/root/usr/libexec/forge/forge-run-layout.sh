#!/usr/bin/env bash
# Recreate /run directories desktop daemons expect after forge mounts tmpfs on /run.
set -euo pipefail

mkdir -p \
  /run/dbus \
  /run/forge/log \
  /run/user \
  /run/lock \
  /run/log \
  /run/gdm \
  /run/systemd/seats \
  /run/systemd/sessions \
  /run/systemd/users \
  /run/systemd/inhibit \
  /run/systemd/ask-password \
  /run/systemd/machines \
  /run/systemd/shutdown \
  /run/NetworkManager \
  /run/nvidia-persistenced \
  /run/systemd/journal \
  /run/systemd/resolve \
  /run/udev \
  /tmp/.X11-unix 2>/dev/null || true

chmod 0755 /run/dbus 2>/dev/null || true
chmod 0711 /run/gdm 2>/dev/null || true
chmod 1777 /tmp/.X11-unix 2>/dev/null || true

chown root:root /run/dbus 2>/dev/null || true
chown root:gdm /run/gdm 2>/dev/null || true

if getent passwd gdm >/dev/null 2>&1; then
  gdm_uid="$(id -u gdm)"
  mkdir -p "/run/user/${gdm_uid}" 2>/dev/null || true
  chown "gdm:gdm" "/run/user/${gdm_uid}" 2>/dev/null || true
  chmod 0700 "/run/user/${gdm_uid}" 2>/dev/null || true
fi

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

exit 0