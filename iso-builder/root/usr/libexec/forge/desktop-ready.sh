#!/usr/bin/env bash
# Last gate before COSMIC greeter — graphics, SELinux labels, logind seat, systemd1 stub.
set -euo pipefail

LOG=/var/log/forge/desktop-ready.log
mkdir -p /var/log/forge
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) desktop-ready start ==="

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"

dbus_name_owned() {
    local name="$1"
    if command -v busctl >/dev/null 2>&1; then
        busctl --address="$BUS" call org.freedesktop.DBus /org/freedesktop/DBus \
            org.freedesktop.DBus NameHasOwner s "$name" 2>/dev/null | grep -q 'true'
        return $?
    fi
    dbus-send --address="$BUS" --dest=org.freedesktop.DBus --print-reply \
        /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$name" 2>/dev/null \
        | grep -q 'boolean true'
}

if [[ -x /usr/libexec/forge/forge-run-layout.sh ]]; then
  /usr/libexec/forge/forge-run-layout.sh || true
fi

for sig in TERM TERM KILL; do
  pkill "-$sig" plymouthd 2>/dev/null || true
  pkill "-$sig" plymouth 2>/dev/null || true
done
if command -v plymouth >/dev/null 2>&1; then
  plymouth quit 2>/dev/null || true
  plymouth deactivate 2>/dev/null || true
fi

for _ in $(seq 1 50); do
  pgrep -x plymouthd >/dev/null || break
  sleep 0.1
done

for _ in $(seq 1 50); do
  dbus_name_owned org.freedesktop.systemd1 && break
  sleep 0.1
done

mkdir -p /run/systemd/seats
if [[ ! -f /run/systemd/seats/seat0 ]]; then
  cat >/run/systemd/seats/seat0 <<'SEAT'
# Minimal seat for forge boot (logind will overwrite/enrich when ready)
IS_SEAT0=1
CAN_MULTI_SESSION=1
CAN_TTY=1
CAN_GRAPHICAL=1
SEAT
  echo "desktop-ready: synthesized /run/systemd/seats/seat0 with CAN_GRAPHICAL" >>"$LOG"
fi

for _ in $(seq 1 150); do
  [[ -f /run/systemd/seats/seat0 ]] && break
  sleep 0.1
done

for _ in $(seq 1 100); do
  busctl --address="$BUS" get-property org.freedesktop.login1 /org/freedesktop/login1/seat/seat0 \
    org.freedesktop.login1.Seat CanGraphical 2>/dev/null | grep -q 'true' && break
  sleep 0.1
done

for _ in $(seq 1 60); do
  [[ -c /dev/dri/card0 ]] || [[ -c /dev/dri/card1 ]] || [[ -c /dev/dri/card2 ]] && break
  sleep 0.1
done

if [[ -x /usr/libexec/forge/cosmic-greeter-setup.sh ]]; then
  /usr/libexec/forge/cosmic-greeter-setup.sh || true
elif [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
  /usr/libexec/forge/release-graphics.sh || true
fi

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  for _ in $(seq 1 10); do
    /usr/libexec/forge/restorecon-forge.sh || true
    if [[ -f /etc/resolv.conf ]]; then
      ctx="$(stat -c '%C' /etc/resolv.conf 2>/dev/null || true)"
      [[ -z "$ctx" || "$ctx" != *"unlabeled_t"* ]] && break
    fi
    sleep 0.3
  done
fi

if command -v chvt >/dev/null 2>&1; then
  chvt 1 2>/dev/null || true
fi

echo "desktop-ready: finished"
exit 0