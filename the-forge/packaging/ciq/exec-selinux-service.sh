#!/usr/bin/env bash
# Launch a daemon the way systemd does: exec domain transition from *_exec_t.
# init_t cannot use runcon to jump domains — that yields "Permission denied" (exit 126).
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: exec-selinux-service.sh [--user=NAME] BINARY [args...]" >&2
  exit 2
fi

DROP_USER=""
if [[ "${1:-}" == --user=* ]]; then
  DROP_USER="${1#--user=}"
  shift
fi

BINARY="$1"
shift

if [[ ! -x "$BINARY" ]]; then
  echo "exec-selinux-service: not executable: $BINARY" >&2
  exit 127
fi

if [[ -n "$DROP_USER" ]] && command -v setpriv >/dev/null 2>&1 \
    && getent passwd "$DROP_USER" >/dev/null 2>&1; then
  if setpriv --reuid="$DROP_USER" --regid="$DROP_USER" --init-groups true 2>/dev/null; then
    exec setpriv --reuid="$DROP_USER" --regid="$DROP_USER" --init-groups -- \
      "$BINARY" "$@"
  else
    echo "exec-selinux-service: warning: setpriv to $DROP_USER failed (unmapped user in namespace?), running as current user" >&2
  fi
fi

exec "$BINARY" "$@"