#!/usr/bin/env bash
# NetworkManager — domain transition from NetworkManager_exec_t (no runcon).
set -euo pipefail

NM="/usr/sbin/NetworkManager"
[[ -x "$NM" ]] || { echo "start-networkmanager: $NM not found" >&2; exit 127; }

exec /usr/libexec/forge/exec-selinux-service.sh "$NM" "$@"