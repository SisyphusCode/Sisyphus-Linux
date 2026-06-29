#!/usr/bin/env bash
# Launch accounts-daemon — GNOME user list / account services.
set -euo pipefail

for candidate in /usr/libexec/accounts-daemon /usr/libexec/accounts-daemon/accounts-daemon; do
  if [[ -x "$candidate" ]]; then
    exec /usr/libexec/forge/exec-selinux-service.sh "$candidate" "$@"
  fi
done

echo "start-accounts-daemon: accounts-daemon not found" >&2
exit 127