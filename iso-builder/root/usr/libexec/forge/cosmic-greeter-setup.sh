#!/usr/bin/env bash
# Prepare COSMIC greeter DRM/VT on hybrid laptops (Intel panel + discrete GPU).
set -euo pipefail
modprobe -q virtio_gpu virtio_pci drm || true

LOG=/var/log/forge/cosmic-greeter-setup.log
mkdir -p /var/log/forge /run/cosmic-greeter
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) cosmic-greeter-setup start ==="

touch /run/forge/plymouth-disabled 2>/dev/null || true

for sig in TERM TERM KILL; do
  pkill "-$sig" plymouthd 2>/dev/null || true
  pkill "-$sig" plymouth 2>/dev/null || true
done
if command -v plymouth >/dev/null 2>&1; then
  plymouth quit 2>/dev/null || true
  plymouth deactivate 2>/dev/null || true
fi

# udev gdm rules can disable Wayland on hybrid NVIDIA — clear markers for COSMIC.
rm -f /run/udev/gdm-machine-is-laptop \
  /run/udev/gdm-machine-has-hybrid-graphics \
  /run/udev/gdm-machine-has-vendor-nvidia-driver 2>/dev/null || true

pick_drm_cards() {
  local -a connected=()
  local -a fallback=()
  local entry card idx driver

  for entry in /sys/class/drm/card*-*; do
    [[ -d "$entry" ]] || continue
    [[ "$(basename "$entry")" =~ -[0-9]+$ ]] || continue
    [[ -f "$entry/status" ]] || continue
    idx="${entry#/sys/class/drm/card}"
    idx="${idx%%-*}"
    driver=""
    if [[ -f "/sys/class/drm/card${idx}/device/uevent" ]]; then
      driver="$(grep -E '^DRIVER=' "/sys/class/drm/card${idx}/device/uevent" 2>/dev/null | cut -d= -f2 || true)"
    fi
    if [[ "$(cat "$entry/status" 2>/dev/null)" == "connected" ]]; then
      connected+=("card${idx}")
    elif [[ "$driver" == "i915" || "$driver" == "amdgpu" || "$driver" == "xe" ]]; then
      fallback+=("card${idx}")
    fi
  done

  if [[ ${#connected[@]} -gt 0 ]]; then
    printf '%s\n' "${connected[@]}" | awk '!seen[$0]++' | paste -sd, -
    return 0
  fi
  if [[ ${#fallback[@]} -gt 0 ]]; then
    printf '%s\n' "${fallback[@]}" | awk '!seen[$0]++' | paste -sd, -
    return 0
  fi
  for card in /dev/dri/card[0-9]*; do
    [[ -c "$card" ]] || continue
    basename "$card"
  done | paste -sd, -
}

DRM_DEVICES="$(pick_drm_cards || true)"
if [[ -n "$DRM_DEVICES" ]]; then
  echo "cosmic-greeter-setup: WLR_DRM_DEVICES=${DRM_DEVICES}"
  cat >/run/cosmic-greeter/environment <<EOF
WLR_DRM_DEVICES=${DRM_DEVICES}
WLR_NO_HARDWARE_CURSORS=1
LIBSEAT_BACKEND=logind
EOF
  chmod 0644 /run/cosmic-greeter/environment
else
  echo "cosmic-greeter-setup: no DRM devices found yet"
fi

# Fallback when logind device delegation is unavailable under forge PID 1.
if getent passwd cosmic-greeter >/dev/null 2>&1; then
  for dev in /dev/dri/card* /dev/dri/renderD*; do
    [[ -e "$dev" ]] || continue
    chgrp video "$dev" 2>/dev/null || true
    chmod g+rw "$dev" 2>/dev/null || true
    if command -v setfacl >/dev/null 2>&1; then
      setfacl -m "u:cosmic-greeter:rw" "$dev" 2>/dev/null || true
    fi
  done
fi

# Clean stale X11/Wayland sockets from previous crashes
rm -f /tmp/.X[0-9]*-lock /tmp/.X11-unix/X* 2>/dev/null || true
rm -rf /run/user/*/wayland-* /tmp/wayland-* 2>/dev/null || true

if [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
  /usr/libexec/forge/release-graphics.sh || true
fi

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

if command -v chvt >/dev/null 2>&1; then
  chvt 1 2>/dev/null || true
fi

echo "cosmic-greeter-setup: finished"
exit 0