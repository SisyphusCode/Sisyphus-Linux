#!/usr/bin/env bash
# Release initramfs Plymouth/KMS hold and switch to the GDM greeter VT before GDM/Xorg.
set -euo pipefail

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

gdm_vt() {
  local vt="1"
  local f line section
  for f in /etc/gdm/custom.conf /etc/gdm/custom.conf.d/*.conf; do
    [[ -f "$f" ]] || continue
    section=""
    while IFS= read -r line || [[ -n "$line" ]]; do
      line="${line%%#*}"
      line="${line#"${line%%[![:space:]]*}"}"
      line="${line%"${line##*[![:space:]]}"}"
      [[ -z "$line" ]] && continue
      if [[ "$line" =~ ^\[(.+)\]$ ]]; then
        section="${BASH_REMATCH[1]}"
        continue
      fi
      if [[ "$section" == "daemon" && "$line" =~ ^DefaultVT[[:space:]]*=[[:space:]]*([0-9]+) ]]; then
        vt="${BASH_REMATCH[1]}"
      fi
    done <"$f"
  done
  echo "$vt"
}

reset_vt() {
  local n="$1"
  local tty="/dev/tty${n}"
  [[ -c "$tty" ]] || return 0
  if command -v setterm >/dev/null 2>&1; then
    setterm -term linux -reset -blank 0 -powerdown 0 -powersave off </dev/null >"$tty" 2>/dev/null || true
  fi
}

VT="$(gdm_vt)"

for sig in TERM TERM KILL; do
  pkill "-$sig" plymouthd 2>/dev/null || true
  pkill "-$sig" plymouth 2>/dev/null || true
done
if command -v plymouth >/dev/null 2>&1; then
  plymouth quit 2>/dev/null || true
  plymouth deactivate 2>/dev/null || true
fi
for _ in $(seq 1 50); do
  pgrep -x plymouthd >/dev/null || break
  sleep 0.1
done

# Drop any lingering graphics mode on forge greeter VTs before GDM/Xorg claim logind session.
for n in "$VT" 1 2 3; do
  reset_vt "$n"
done
for tty in /dev/tty0 /dev/console; do
  [[ -c "$tty" ]] || continue
  if command -v setterm >/dev/null 2>&1; then
    setterm -term linux -reset </dev/null >"$tty" 2>/dev/null || true
  fi
done

if command -v chvt >/dev/null 2>&1; then
  chvt "$VT" 2>/dev/null || true
fi

exit 0