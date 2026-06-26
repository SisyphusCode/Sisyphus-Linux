#!/usr/bin/env bash
# NetworkManager dispatcher — relabel resolv.conf after NM rewrites it (forge PID 1).
set -euo pipefail

[[ "${2:-}" == "up" || "${2:-}" == "dhcp4-change" || "${2:-}" == "connectivity-change" ]] || exit 0

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

exit 0