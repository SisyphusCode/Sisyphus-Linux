#!/usr/bin/env bash
# Resolve agetty on Rocky/CIQ (/usr/sbin vs /sbin) and exec with unit args.
set -euo pipefail

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