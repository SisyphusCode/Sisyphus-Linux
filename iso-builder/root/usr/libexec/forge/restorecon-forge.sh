#!/usr/bin/env bash
# Relabel files NetworkManager/GDM create before SELinux enforcing daemons read them.
set -euo pipefail

relabel_path() {
  local p="$1"
  [[ -e "$p" ]] || return 0

  if command -v restorecon >/dev/null 2>&1; then
    restorecon -F "$p" 2>/dev/null || restorecon "$p" 2>/dev/null || true
  fi

  local current=""
  current="$(stat -c '%C' "$p" 2>/dev/null || true)"
  [[ -n "$current" ]] || return 0
  [[ "$current" != *"unlabeled_t"* ]] && return 0

  if command -v matchpathcon >/dev/null 2>&1; then
    local expected
    expected="$(matchpathcon -n "$p" 2>/dev/null || true)"
    if [[ -n "$expected" ]]; then
      chcon "$expected" "$p" 2>/dev/null || true
      current="$(stat -c '%C' "$p" 2>/dev/null || true)"
      [[ "$current" != *"unlabeled_t"* ]] && return 0
    fi
  fi

  case "$p" in
    /etc/resolv.conf) chcon -t net_conf_t "$p" 2>/dev/null || true ;;
    /etc/hostname) chcon -t hostname_etc_t "$p" 2>/dev/null || true ;;
    /etc/hosts) chcon -t net_conf_t "$p" 2>/dev/null || true ;;
    /etc/machine-id) chcon -t etc_t "$p" 2>/dev/null || true ;;
  esac
}

relabel_tree() {
  local d="$1"
  [[ -d "$d" ]] || return 0
  if command -v restorecon >/dev/null 2>&1; then
    restorecon -R "$d" 2>/dev/null || true
  fi
}

for p in /etc/resolv.conf /etc/hostname /etc/hosts /etc/machine-id; do
  relabel_path "$p"
done

for d in \
  /var/lib/NetworkManager \
  /var/lib/dhcp \
  /var/lib/gdm \
  /var/log/gdm \
  /var/log/forge \
  /run/NetworkManager \
  /run/dbus \
  /run/systemd \
  /run/gdm \
  /run/log \
  /run/user \
  /tmp/.X11-unix; do
  relabel_tree "$d"
done

# NM may rewrite resolv.conf after the tree pass — final explicit relabel.
relabel_path /etc/resolv.conf

exit 0