#!/bin/sh
# Register a logind session when forge-core is PID 1 (pam_systemd requires systemd as init).
set -e

pid1="$(ps -o comm= -p 1 2>/dev/null || true)"
[ "$pid1" = "forge-core" ] || exit 0

user="${PAM_USER:-}"
[ -n "$user" ] || exit 0

uid="$(id -u "$user" 2>/dev/null || true)"
[ -n "$uid" ] || exit 0

runtime="/run/user/${uid}"
mkdir -p "$runtime" 2>/dev/null || true
chown "${user}:${user}" "$runtime" 2>/dev/null || true
chmod 0700 "$runtime" 2>/dev/null || true

if [ -x /usr/libexec/forge/restorecon-forge.sh ]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

# pam_selinux.so open needs a logind session — pam_systemd is a no-op under forge PID 1.
if command -v python3 >/dev/null 2>&1; then
  PAM_USER="$user" PAM_UID="$uid" PAM_SERVICE="${PAM_SERVICE:-login}" PAM_TTY="${PAM_TTY:-}" \
    python3 /usr/libexec/forge/pam-logind-create-session.py >/dev/null 2>&1 || true
fi

exit 0