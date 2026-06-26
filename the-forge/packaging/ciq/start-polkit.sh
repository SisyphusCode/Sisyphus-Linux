#!/usr/bin/env bash
# polkitd — domain transition from polkitd_exec_t; drops to user polkitd internally.
set -euo pipefail

POLKITD=""
for candidate in /usr/lib/polkit-1/polkitd /usr/libexec/polkitd; do
  if [[ -x "$candidate" ]]; then
    POLKITD="$candidate"
    break
  fi
done

if [[ -z "$POLKITD" ]]; then
  echo "start-polkit: polkitd not found (install polkit)" >&2
  exit 127
fi

# Run like the real polkit.service: as polkitd user, with standard flags for no debug + log level.
# The exec-selinux wrapper will handle setpriv drop if possible.
exec /usr/libexec/forge/exec-selinux-service.sh --user=polkitd "$POLKITD" --no-debug --log-level=err "$@"