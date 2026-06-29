#!/usr/bin/env bash
# Quick one-shot kill for initramfs Plymouth (native forge worker does ongoing poll in background).
# Keep this short so the oneshot doesn't block boot waves.
set -euo pipefail

LOG=/var/log/forge/plymouth-kill.log
mkdir -p /var/log/forge /run/forge
touch /run/forge/plymouth-disabled 2>/dev/null || true

for sig in TERM TERM KILL; do
  pkill "-$sig" plymouthd 2>/dev/null || true
  pkill "-$sig" plymouth 2>/dev/null || true
done
if command -v plymouth >/dev/null 2>&1; then
  plymouth quit 2>/dev/null || true
  plymouth deactivate 2>/dev/null || true
fi

echo "$(date -Is 2>/dev/null || date) plymouth-kill oneshot done (native worker continues)" >>"$LOG" 2>/dev/null || true
exit 0
