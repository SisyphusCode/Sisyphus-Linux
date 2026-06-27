#!/usr/bin/env bash
# Resolve agetty and exec with unit args, but gracefully exit if device doesn't exist.
set -euo pipefail

LOG=/var/log/forge/getty-debug.log
echo "=== start-agetty.sh called with args: $* ===" >> "$LOG" 2>&1

# If the arguments specify a tty but it doesn't exist on this hardware,
# exit gracefully with 0 so forge-core doesn't infinitely restart it on failure.
for arg in "$@"; do
  if [[ "$arg" =~ ^tty ]]; then
    tty_dev="/dev/${arg}"
    if [[ ! -c "$tty_dev" ]]; then
      echo "start-agetty: $tty_dev does not exist, exiting gracefully."
      exit 0
    fi
    # Also check if we can actually use the tty (not locked/accessible)
    if ! timeout 1 bash -c ">> ${tty_dev} 2>&1" >/dev/null 2>&1; then
      echo "start-agetty: $tty_dev exists but is not writable/accessible, exiting gracefully."
      exit 0
    fi
  fi
  if [[ "$arg" == "ttyS0" ]] && [[ ! -c "/dev/ttyS0" ]]; then
    echo "start-agetty: /dev/ttyS0 does not exist, exiting gracefully."
    exit 0
  fi
done

for candidate in /usr/sbin/agetty /sbin/agetty; do
  if [[ -x "$candidate" ]]; then
    exec "$candidate" "$@"
  fi
done

if command -v agetty >/dev/null 2>&1; then
  exec "$(command -v agetty)" "$@"
fi

echo "start-agetty: agetty not found (install util-linux)" >&2
exit 127
