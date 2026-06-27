#!/usr/bin/env bash
# Block dependents until org.freedesktop.systemd1 is registered on the bus.
set -euo pipefail

LOG=/run/forge/systemd1-probe.log
mkdir -p /run/forge /var/log/forge
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

{
    echo "=== $(date -Is 2>/dev/null || date) wait-for-systemd1 ppid=$PPID pid=$$ ==="
    echo "DBUS_SYSTEM_BUS_ADDRESS=$BUS"
} >>"$LOG"

for i in $(seq 1 50); do
    [[ -S /run/dbus/system_bus_socket ]] || { sleep 0.1; continue; }
    if dbus_name_owned org.freedesktop.systemd1; then
        echo "systemd1 registered after ${i} attempts" >>"$LOG"
        exit 0
    fi
    if (( i % 50 == 0 )); then
        echo "still waiting for org.freedesktop.systemd1 (attempt $i)" >>"$LOG"
    fi
    sleep 0.1
done

echo "timeout waiting for org.freedesktop.systemd1" >>"$LOG"
exit 1