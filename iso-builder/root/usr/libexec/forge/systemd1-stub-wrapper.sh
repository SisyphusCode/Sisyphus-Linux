#!/usr/bin/env bash
# Launched by forge PID 1 — log to tmpfs to avoid overlay btrfs I/O errors during early boot.
set -euo pipefail

LOG=/run/forge/systemd1-stub.log
mkdir -p /run/forge /var/log/forge
echo "=== $(date -Is 2>/dev/null || date) systemd1-stub-wrapper pid=$$ ppid=$PPID ===" | tee -a "$LOG" /var/log/forge/systemd1-stub.log 2>/dev/null || true
echo "env DBUS_SYSTEM_BUS_ADDRESS=${DBUS_SYSTEM_BUS_ADDRESS:-unset}" >>"$LOG"

exec /usr/bin/python3 -u /usr/libexec/forge/systemd1-stub.py >>"$LOG" 2>&1