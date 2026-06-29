#!/usr/bin/env bash
# GDM on Rocky/CIQ — domain transition from xdm_exec_t; needs logind seat0.
set -euo pipefail

LOG=/var/log/forge/gdm.log
mkdir -p /var/log/forge
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) start-gdm ==="

GDM=""
for candidate in /usr/sbin/gdm /usr/bin/gdm; do
  if [[ -x "$candidate" ]]; then
    GDM="$candidate"
    break
  fi
done

if [[ -z "$GDM" ]]; then
  if command -v gdm >/dev/null 2>&1; then
    GDM="$(command -v gdm)"
  fi
fi

if [[ -z "$GDM" ]]; then
  echo "start-gdm: gdm not found (install the gdm package)" >&2
  exit 127
fi

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"
for _ in $(seq 1 150); do
  [[ -f /run/systemd/seats/seat0 ]] && break
  sleep 0.1
done
for _ in $(seq 1 100); do
  busctl --address="$BUS" status org.freedesktop.login1 >/dev/null 2>&1 && break
  sleep 0.1
done

if [[ -x /usr/libexec/forge/gdm-greeter-setup.sh ]]; then
  /usr/libexec/forge/gdm-greeter-setup.sh || true
elif [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
  /usr/libexec/forge/release-graphics.sh || true
fi

for _ in $(seq 1 100); do
  busctl --address="$BUS" get-property org.freedesktop.login1 /org/freedesktop/login1/seat/seat0 \
    org.freedesktop.login1.Seat CanGraphical 2>/dev/null | grep -q 'true' && break
  sleep 0.1
done

for _ in $(seq 1 150); do
  busctl --address="$BUS" status org.freedesktop.systemd1 >/dev/null 2>&1 && break
  sleep 0.1
done

for _ in $(seq 1 200); do
  [[ -S /run/dbus/system_bus_socket ]] && break
  sleep 0.1
done
for _ in $(seq 1 100); do
  busctl --address="$BUS" status org.freedesktop.DBus >/dev/null 2>&1 && break
  sleep 0.1
done
for _ in $(seq 1 100); do
  busctl --address="$BUS" status org.freedesktop.login1 >/dev/null 2>&1 && break
  sleep 0.1
done

for _ in $(seq 1 30); do
  if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
    /usr/libexec/forge/restorecon-forge.sh || true
  fi
  if [[ -f /etc/resolv.conf ]]; then
    ctx="$(stat -c '%C' /etc/resolv.conf 2>/dev/null || true)"
    [[ -z "$ctx" || "$ctx" != *"unlabeled_t"* ]] && break
  fi
  sleep 0.2
done

export GDM_DEBUG=1
export GDK_BACKEND=x11

exec /usr/libexec/forge/exec-selinux-service.sh "$GDM" "$@"