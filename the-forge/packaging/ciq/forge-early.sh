#!/usr/bin/env bash
# Early PID 1 setup: runtime dirs, SELinux labels, console nodes, CIQ modules.
set -euo pipefail

if [[ -x /usr/libexec/forge/forge-run-layout.sh ]]; then
  /usr/libexec/forge/forge-run-layout.sh || true
fi

mkdir -p /var/log/forge /var/lib/dbus /var/tmp 2>/dev/null || true

rm -f /run/dbus/system_bus_socket /run/dbus/pid /run/nologin 2>/dev/null || true

# Keep root:root on /run/dbus (required for runcon system_dbusd_t socket bind on CIQ).
chown root:root /run/dbus 2>/dev/null || true
chmod 0755 /run/dbus 2>/dev/null || true
chown root:gdm /run/gdm 2>/dev/null || true
chmod 0711 /run/gdm 2>/dev/null || true

if command -v dbus-uuidgen >/dev/null 2>&1; then
  dbus-uuidgen --ensure=/var/lib/dbus/machine-id 2>/dev/null || true
fi

# SELinux policy + enforce mode are handled in forge-core vfs.rs before units start.

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi
if command -v chcon >/dev/null 2>&1; then
  chcon -u system_u -r object_r -t system_dbusd_var_run_t /run/dbus 2>/dev/null \
    || chcon -t system_dbusd_var_run_t /run/dbus 2>/dev/null || true
  # SELinux labels for logind runtime (systemd-logind or elogind variants)
  chcon -u system_u -r object_r -t systemd_logind_var_run_t \
    /run/systemd/seats /run/systemd/sessions /run/systemd/users 2>/dev/null || true
  chcon -u system_u -r object_r -t elogind_var_run_t \
    /run/systemd/seats /run/systemd/sessions /run/systemd/users 2>/dev/null || true
fi

# Stale daemons from initramfs or partial boots must not run before runtime dirs exist.
if [[ -z "${FORGE_MOCK_BOOT:-}" ]] && [[ "$(ps -o comm= -p 1 2>/dev/null || true)" == "forge-core" ]]; then
  # Initramfs leaves these running against a dead /run — breaks forge dbus/logind.
  pkill -9 dbus-daemon 2>/dev/null || true
  pkill -9 dbus-broker 2>/dev/null || true
  pkill -9 systemd-logind 2>/dev/null || true
  pkill -9 elogind 2>/dev/null || true
  pkill -9 systemd-udevd 2>/dev/null || true
  pkill -9 udevd 2>/dev/null || true
  pkill -9 plymouthd 2>/dev/null || true
  pkill -9 plymouth 2>/dev/null || true
fi

for n in 0 1 2 3; do
  [[ -c "/dev/tty$n" ]] || mknod -m 622 "/dev/tty$n" c 4 "$n" 2>/dev/null || true
done
[[ -c /dev/console ]] || mknod -m 622 /dev/console c 5 1 2>/dev/null || true
[[ -c /dev/null ]] || mknod -m 666 /dev/null c 1 3 2>/dev/null || true

for m in i8042 atkbd usbhid iwlwifi nvidia nvidia_drm nvidia_modeset drm; do
  modprobe "$m" 2>/dev/null || true
done

if getent passwd gdm >/dev/null 2>&1; then
  gdm_uid="$(id -u gdm)"
  mkdir -p "/run/user/${gdm_uid}" 2>/dev/null || true
  chown "gdm:gdm" "/run/user/${gdm_uid}" 2>/dev/null || true
  chmod 0700 "/run/user/${gdm_uid}" 2>/dev/null || true
fi

# GDM/Xorg need an X11 socket dir; gdm cannot create /tmp/.X11-unix as non-root.
mkdir -p /tmp/.X11-unix 2>/dev/null || true
chmod 1777 /tmp/.X11-unix 2>/dev/null || true

# Plymouth from initramfs may still own the VT/KMS until explicitly quit.
if [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
  /usr/libexec/forge/release-graphics.sh || true
fi

# VT access for GDM or forge-desktop auto-login (Xorg opens /dev/tty0).
if command -v setfacl >/dev/null 2>&1; then
  desktop_user="Sisyphus"
  if [[ -f /etc/forge/desktop.toml ]]; then
    line="$(grep -E '^[[:space:]]*user[[:space:]]*=' /etc/forge/desktop.toml | head -1 || true)"
    [[ -n "$line" ]] && desktop_user="${line#*=}" && desktop_user="${desktop_user// /}"
    desktop_user="${desktop_user%\"}"; desktop_user="${desktop_user#\"}"
  fi
  for vt in /dev/tty0 /dev/tty1 /dev/tty2 /dev/tty3 /dev/console; do
    [[ -e "$vt" ]] || continue
    getent passwd gdm >/dev/null 2>&1 \
      && setfacl -m "u:gdm:rw,m::rw" "$vt" 2>/dev/null || true
    id "$desktop_user" >/dev/null 2>&1 \
      && setfacl -m "u:${desktop_user}:rw,m::rw" "$vt" 2>/dev/null || true
  done
fi

exit 0