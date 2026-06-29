#!/usr/bin/env bash
# Start elogind or systemd-logind under Forge PID 1.
# Auto-detects the binary. Handles SELinux wrapper if present.
set -euo pipefail

# Support standalone elogind (common on non-systemd distros for GUI) and systemd-logind.
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
        break
    fi
done

# Fallback to PATH
if [[ -z "$LOGIND" ]]; then
    for bin in elogind systemd-logind; do
        if command -v "$bin" >/dev/null 2>&1; then
            LOGIND="$(command -v "$bin")"
            break
        fi
    done
fi

[[ -n "$LOGIND" ]] || {
    echo "start-logind: no elogind or systemd-logind binary found in common paths or PATH" >&2
    exit 127
}

echo "start-logind: using $LOGIND" >&2

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"
for _ in $(seq 1 150); do
  if command -v busctl >/dev/null 2>&1; then
    busctl --address="$BUS" status org.freedesktop.systemd1 >/dev/null 2>&1 && break
  fi
  sleep 0.1
done

WRAPPER="/usr/libexec/forge/exec-selinux-service.sh"
if [[ -x "$WRAPPER" ]]; then
    exec "$WRAPPER" "$LOGIND" "$@"
else
    exec "$LOGIND" "$@"
fi