#!/usr/bin/bash
# systemd-udevd for PID 1 — initramfs may leave a stale instance; stop it first.
set -euo pipefail

WRAPPER_LOG=/var/log/forge/udev-wrapper.log
mkdir -p /var/log/forge
echo "=== $(date -Is 2>/dev/null || date) start-udevd ppid=$PPID pid=$$ LISTEN_FDS=${LISTEN_FDS:-0} ===" >>"$WRAPPER_LOG"
echo "Open FDs in start-udevd.sh:" >>"$WRAPPER_LOG"
ls -l /proc/$$/fd >>"$WRAPPER_LOG" 2>&1 || true

if [[ "$(ps -o comm= -p 1 2>/dev/null || true)" == "forge-core" ]]; then
  if [[ ! -f /run/forge/udevd-stale-killed ]]; then
    mkdir -p /run/forge
    pkill -9 systemd-udevd 2>/dev/null || true
    pkill -9 udevd 2>/dev/null || true
    touch /run/forge/udevd-stale-killed
    sleep 0.2
  fi
fi

# Ensure LISTEN_PID is set to our current PID for systemd-style socket activation
export LISTEN_PID=$$
export LISTEN_FDNAMES=systemd-udevd-control:systemd-udevd-kernel
echo "Exported LISTEN_PID=$LISTEN_PID LISTEN_FDNAMES=$LISTEN_FDNAMES" >>"$WRAPPER_LOG"

exec /usr/lib/systemd/systemd-udevd "$@"