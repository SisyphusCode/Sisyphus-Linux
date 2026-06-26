#!/usr/bin/env bash
# Session-bus org.freedesktop.systemd1 — launched by dbus-daemon --session activation.
uid="$(id -u)"
runtime="${XDG_RUNTIME_DIR:-/run/user/${uid}}"
LOG="/tmp/systemd1-session-stub-${uid}.log"
if mkdir -p "$runtime" 2>/dev/null && touch "${runtime}/systemd1-session-stub.log" 2>/dev/null; then
  LOG="${runtime}/systemd1-session-stub.log"
fi

{
  echo "=== $(date -Is 2>/dev/null || date) session-stub-wrapper pid=$$ ppid=$PPID uid=$uid ==="
  echo "env DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unset} DBUS_STARTER_ADDRESS=${DBUS_STARTER_ADDRESS:-unset} DBUS_STARTER_BUS_TYPE=${DBUS_STARTER_BUS_TYPE:-unset}"
} >>"$LOG" 2>/dev/null || true

exec /usr/bin/python3 /usr/libexec/forge/systemd1-session-stub.py >>"$LOG" 2>&1