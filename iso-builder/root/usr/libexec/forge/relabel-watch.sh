#!/usr/bin/env bash
# Keep NM/GDM-created files labeled while forge is PID 1 (background).
set -euo pipefail

[[ "$(ps -o comm= -p 1 2>/dev/null || true)" == "forge-core" ]] || exit 0

LOG=/var/log/forge/relabel-watch.log
mkdir -p /var/log/forge

end=$((SECONDS + 180))
while (( SECONDS < end )); do
  if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
    /usr/libexec/forge/restorecon-forge.sh >>"$LOG" 2>&1 || true
  fi
  sleep 3
done
exit 0