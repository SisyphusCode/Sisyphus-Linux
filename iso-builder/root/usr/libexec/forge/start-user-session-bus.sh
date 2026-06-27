#!/usr/bin/env bash
# Start per-user session dbus + systemd1 session stub (Forge PID 1).
set -euo pipefail

uid="${1:-}"
[[ -n "$uid" ]] || { echo "start-user-session-bus: missing uid" >&2; exit 1; }

pw="$(getent passwd "$uid" | cut -d: -f1,3,4,6 || true)"
[[ -n "$pw" ]] || { echo "start-user-session-bus: unknown uid $uid" >&2; exit 1; }
user="$(echo "$pw" | cut -d: -f1)"
gid="$(echo "$pw" | cut -d: -f3)"
home="$(echo "$pw" | cut -d: -f4)"

runtime="/run/user/${uid}"
bus="${runtime}/bus"

mkdir -p "$runtime"
chown "${uid}:${gid}" "$runtime"
chmod 0700 "$runtime"

if [[ -S "$bus" ]]; then
    exit 0
fi

export XDG_RUNTIME_DIR="$runtime"
export DBUS_SESSION_BUS_ADDRESS="unix:path=${bus}"
export HOME="$home"
export USER="$user"
export LOGNAME="$user"

/usr/bin/dbus-daemon \
    --session \
    --address="unix:path=${bus}" \
    --nopidfile \
    --nofork \
    </dev/null >/dev/null 2>&1 &
dbus_pid=$!

for _ in $(seq 1 50); do
    [[ -S "$bus" ]] && break
    kill -0 "$dbus_pid" 2>/dev/null || break
    sleep 0.1
done

[[ -S "$bus" ]] || {
    echo "start-user-session-bus: session bus missing at ${bus}" >&2
    kill "$dbus_pid" 2>/dev/null || true
    exit 1
}

chown "${uid}:${gid}" "$bus" 2>/dev/null || true

setsid /usr/libexec/forge/systemd1-session-stub-wrapper.sh \
    </dev/null >/dev/null 2>&1 &

exit 0