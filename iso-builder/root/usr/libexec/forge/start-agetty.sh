#!/usr/bin/env bash
# Resolve agetty and exec with unit args, but gracefully exit if device doesn't exist.
set -euo pipefail

# If the arguments specify ttyS0 but it doesn't exist on this hardware,
# exit gracefully with 0 so forge-core doesn't infinitely restart it on failure.
for arg in "$@"; do
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
