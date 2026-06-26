#!/usr/bin/env bash
# Launched by forge PID 1 (not dbus activation) — logs failures for post-mortem.
set -euo pipefail

LOG=/var/log/forge/systemd1-stub.log
mkdir -p /var/log/forge
echo "=== $(date -Is 2>/dev/null || date) systemd1-stub-wrapper pid=$$ ppid=$PPID ===" >>"$LOG"
echo "env DBUS_SYSTEM_BUS_ADDRESS=${DBUS_SYSTEM_BUS_ADDRESS:-unset}" >>"$LOG"

exec /usr/bin/python3 /usr/libexec/forge/systemd1-stub.py >>"$LOG" 2>&1