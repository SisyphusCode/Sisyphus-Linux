#!/usr/bin/env bash
# CIQ RLC Pro / Rocky GNOME network bring-up for The Forge.
# Wi-Fi interfaces appear late; this script exits immediately and continues in the background.
set -uo pipefail

IFACE="${FORGE_NET_IFACE:-}"
TIMEOUT="${FORGE_NET_TIMEOUT:-60}"
LOG="${FORGE_NET_LOG:-/run/forge/log/network-setup.log}"

log() { echo "network-setup: $*" | tee -a "$LOG" >&2; }

ip_bin() {
  for candidate in /sbin/ip /usr/sbin/ip /bin/ip /usr/bin/ip; do
    [[ -x "$candidate" ]] && { echo "$candidate"; return 0; }
  done
  echo "ip"
}

IP="$(ip_bin)"

detect_wifi_iface() {
  if [[ -n "$IFACE" ]] && "$IP" link show "$IFACE" &>/dev/null; then
    echo "$IFACE"
    return 0
  fi
  if command -v nmcli >/dev/null 2>&1; then
    local dev
    dev="$(nmcli -t -f DEVICE,TYPE device 2>/dev/null | awk -F: '$2 == "wifi" { print $1; exit }')"
    if [[ -n "$dev" ]]; then
      echo "$dev"
      return 0
    fi
  fi
  local guessed
  guessed="$("$IP" -o link show 2>/dev/null | awk -F': ' '/: wl/ { gsub("@.*","",$2); print $2; exit }')"
  if [[ -n "$guessed" ]]; then
    echo "$guessed"
    return 0
  fi
  echo "${IFACE:-wlp82s0}"
}

load_wifi_modules() {
  for mod in iwlwifi iwlmvm cfg80211 mac80211 brcmfmac; do
    modprobe "$mod" 2>/dev/null || true
  done
}

wait_for_iface() {
  local i
  for ((i = 1; i <= TIMEOUT; i++)); do
    if "$IP" link show "$IFACE" &>/dev/null; then
      log "interface '$IFACE' appeared after ${i}s"
      return 0
    fi
    sleep 1
  done
  log "interface '$IFACE' not found after ${TIMEOUT}s"
  return 1
}

wait_for_network_manager() {
  command -v busctl >/dev/null 2>&1 || return 0
  local i
  for ((i = 1; i <= 45; i++)); do
    if busctl --timeout=1 status org.freedesktop.NetworkManager &>/dev/null; then
      log "NetworkManager bus ready after ${i}s"
      return 0
    fi
    sleep 1
  done
  log "NetworkManager bus not ready after 45s"
  return 1
}

nm_connected() {
  local state
  state="$(nmcli -t -f DEVICE,STATE device 2>/dev/null | awk -F: -v d="$IFACE" '$1 == d { print $2; exit }')"
  [[ "$state" == "connected" ]]
}

wait_nm_connected() {
  local i
  for ((i = 1; i <= TIMEOUT; i++)); do
    if nm_connected; then
      log "interface '$IFACE' connected via NetworkManager"
      return 0
    fi
    sleep 1
  done
  return 1
}

bring_up_wifi() {
  command -v nmcli >/dev/null 2>&1 || return 1
  nmcli radio wifi on 2>/dev/null || true
  nmcli networking on 2>/dev/null || true

  if wait_nm_connected; then
    return 0
  fi

  # Try activating any saved connection bound to this device.
  local con
  con="$(nmcli -t -f NAME,DEVICE connection show --active 2>/dev/null | awk -F: -v d="$IFACE" '$2 == d { print $1; exit }')"
  if [[ -z "$con" ]]; then
    con="$(nmcli -t -f NAME,DEVICE connection show 2>/dev/null | awk -F: -v d="$IFACE" '$2 == d { print $1; exit }')"
  fi
  if [[ -n "$con" ]]; then
    log "activating saved connection '$con' on '$IFACE'"
    nmcli connection up id "$con" 2>/dev/null && wait_nm_connected && return 0
  fi

  # Last resort: ask NM to autoconnect the device.
  nmcli device connect "$IFACE" 2>/dev/null || true
  wait_nm_connected
}

bring_up_ethernet() {
  command -v nmcli >/dev/null 2>&1 || return 1
  nmcli networking on 2>/dev/null || true
  log "nmcli device connect '$IFACE'"
  nmcli device connect "$IFACE" 2>/dev/null || return 1
  wait_nm_connected
}

bring_up_dhcp() {
  if command -v udhcpc >/dev/null 2>&1; then
    log "udhcpc on '$IFACE'"
    udhcpc -i "$IFACE" -q -b
    return 0
  fi
  if command -v dhclient >/dev/null 2>&1; then
    log "dhclient on '$IFACE'"
    dhclient -v "$IFACE"
    return 0
  fi
  return 1
}

background_worker() {
  mkdir -p "$(dirname "$LOG")"
  : >"$LOG"
  IFACE="$(detect_wifi_iface)"
  log "background worker started (iface=$IFACE)"

  load_wifi_modules
  wait_for_iface || exit 0
  "$IP" link set "$IFACE" up 2>/dev/null || true
  wait_for_network_manager || true

  local kind
  kind="$(nmcli -t -f DEVICE,TYPE device 2>/dev/null | awk -F: -v d="$IFACE" '$1 == d { print $2; exit }')"
  case "${kind:-wifi}" in
    wifi|wlan) bring_up_wifi || true ;;
    ethernet|*) bring_up_ethernet || bring_up_dhcp || true ;;
  esac

  if nm_connected; then
    log "network online"
    if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
      /usr/libexec/forge/restorecon-forge.sh || true
    fi
  else
    log "network bring-up incomplete (saved Wi-Fi profile may be required)"
  fi
}

main() {
  mkdir -p "$(dirname "$LOG")"
  background_worker >>"$LOG" 2>&1 &
  log "spawned background network worker (pid $!)"
  exit 0
}

main "$@"