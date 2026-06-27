#!/usr/bin/bash
# Start elogind or systemd-logind under Forge PID 1.
set -euo pipefail

LOG=/var/log/forge/logind-wrapper.log
mkdir -p /var/log/forge
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) start-logind ppid=$PPID pid=$$ NOTIFY_SOCKET=${NOTIFY_SOCKET:-unset} ==="
echo "DEBUG: Checking for logind binary..."
LOGIND=""
candidates=(
    /usr/libexec/elogind/elogind
    /usr/lib/elogind/elogind
    /usr/lib64/elogind/elogind
    /usr/sbin/elogind
    /usr/bin/elogind
    /usr/lib/systemd/systemd-logind
    /lib/systemd/systemd-logind
    /usr/lib64/systemd/systemd-logind
)
for candidate in "${candidates[@]}"; do
    if [[ -x "$candidate" ]]; then
        LOGIND="$candidate"
        echo "DEBUG: Found logind at $LOGIND"
        break
    fi
done
if [[ -z "$LOGIND" ]]; then
    echo "DEBUG: ERROR - no logind binary found, trying command -v"
    for bin in elogind systemd-logind; do
        if command -v "$bin" >/dev/null 2>&1; then
            LOGIND="$(command -v "$bin")"
            echo "DEBUG: Found logind via command -v: $LOGIND"
            break
        fi
    done
fi
[[ -n "$LOGIND" ]] || {
    echo "DEBUG: ERROR - no elogind or systemd-logind binary found" >&2
    exit 127
}
echo "DEBUG: Using logind: $LOGIND"

echo "start-logind: using $LOGIND"

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

for i in $(seq 1 50); do
    if dbus_name_owned org.freedesktop.systemd1; then
        echo "systemd1 registered before logind exec (attempt $i)"
        break
    fi
    sleep 0.1
done

if ! dbus_name_owned org.freedesktop.systemd1; then
    echo "start-logind: org.freedesktop.systemd1 not registered — refusing to start logind"
    exit 1
fi

WRAPPER="/usr/libexec/forge/exec-selinux-service.sh"
if [[ -x "$WRAPPER" ]]; then
    exec "$WRAPPER" "$LOGIND" "$@"
else
    exec "$LOGIND" "$@"
fi