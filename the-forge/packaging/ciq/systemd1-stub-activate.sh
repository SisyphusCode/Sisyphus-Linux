#!/usr/bin/env bash
# dbus-daemon activation entry — succeed if forge already owns org.freedesktop.systemd1.
set -euo pipefail

LOG=/var/log/forge/systemd1-stub.log
mkdir -p /var/log/forge

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"
if command -v busctl >/dev/null 2>&1; then
  if busctl --address="$BUS" status org.freedesktop.systemd1 >/dev/null 2>&1; then
    echo "systemd1-stub-activate: name already on bus" >>"$LOG"
    exit 0
  fi
fi

echo "systemd1-stub-activate: starting stub via wrapper" >>"$LOG"
exec /usr/libexec/forge/systemd1-stub-wrapper.sh