#!/usr/bin/env bash
# Prepare COSMIC greeter DRM/VT on hybrid laptops (Intel panel + discrete GPU).
set -euo pipefail
modprobe -q virtio_gpu virtio_pci drm || true

LOG=/var/lib/forge/cosmic-greeter-setup.log
mkdir -p "$(dirname "$LOG")" /run/cosmic-greeter
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) cosmic-greeter-setup start ==="
rm -f /run/cosmic-greeter/environment

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

is_virtualized() {
  if command -v systemd-detect-virt >/dev/null 2>&1; then
    systemd-detect-virt --quiet && return 0
  fi
  grep -q 'hypervisor' /proc/cpuinfo 2>/dev/null && return 0
  for hint in /sys/class/dmi/id/product_name /sys/class/dmi/id/sys_vendor; do
    [[ -r "$hint" ]] || continue
    if grep -qiE 'kvm|qemu|vmware|virtualbox|xen|hyper-v|bhyve' "$hint"; then
      return 0
    fi
  done
  return 1
}

can_open_drm() {
  local card="$1"
  [[ -c "$card" ]] || return 1
  if getent passwd cosmic-greeter >/dev/null 2>&1; then
    runuser -u cosmic-greeter -- sh -c "exec 9<>\"$card\"; exec 9>&-" >/dev/null 2>&1 && return 0
  fi
  if exec 9<>"$card" 2>/dev/null; then
    exec 9>&-
    return 0
  fi
  return 1
}

pick_drm_cards() {
  local -a connected=()
  local -a fallback=()
  local entry idx driver card

  if is_virtualized; then
    for card in /dev/dri/card0 /dev/dri/card1 /dev/dri/card2 /dev/dri/card3; do
      can_open_drm "$card" && echo "$card" && return 0
    done
  fi

  for entry in /sys/class/drm/card*-*; do
    [[ -d "$entry" ]] || continue
    [[ "$(basename "$entry")" =~ -[0-9]+$ ]] || continue
    [[ -f "$entry/status" ]] || continue
    idx="${entry#/sys/class/drm/card}"
    idx="${idx%%-*}"
    card="/dev/dri/card${idx}"
    can_open_drm "$card" || continue
    driver=""
    if [[ -f "/sys/class/drm/card${idx}/device/uevent" ]]; then
      driver="$(grep -E '^DRIVER=' "/sys/class/drm/card${idx}/device/uevent" 2>/dev/null | cut -d= -f2 || true)"
    fi
    if [[ "$(cat "$entry/status" 2>/dev/null)" == "connected" ]]; then
      connected+=("$card")
    elif [[ "$driver" == "i915" || "$driver" == "amdgpu" || "$driver" == "xe" ]]; then
      fallback+=("$card")
    fi
  done

  if [[ ${#connected[@]} -gt 0 ]]; then
    printf '%s\n' "${connected[@]}" | awk '!seen[$0]++' | paste -sd: -
    return 0
  fi
  if [[ ${#fallback[@]} -gt 0 ]]; then
    printf '%s\n' "${fallback[@]}" | awk '!seen[$0]++' | paste -sd: -
    return 0
  fi
  for card in /dev/dri/card[0-9]*; do
    can_open_drm "$card" && echo "$card"
  done | paste -sd: -
}

DRM_DEVICES="$(pick_drm_cards || true)"
if [[ -n "$DRM_DEVICES" ]]; then
  echo "cosmic-greeter-setup: WLR_DRM_DEVICES=${DRM_DEVICES}"
  {
    echo "WLR_DRM_DEVICES=${DRM_DEVICES}"
    echo "WLR_NO_HARDWARE_CURSORS=1"
    if command -v seatd >/dev/null 2>&1; then
      echo "LIBSEAT_BACKEND=seatd"
      echo "SEATD_SOCK=/run/seatd.sock"
    fi
    if is_virtualized; then
      echo "WLR_RENDERER=pixman"
    fi
  } > /run/cosmic-greeter/environment
  chmod 0644 /run/cosmic-greeter/environment
else
  echo "cosmic-greeter-setup: no usable DRM devices found yet"
fi

if getent passwd cosmic-greeter >/dev/null 2>&1; then
  cg_uid="$(id -u cosmic-greeter)"
  mkdir -p /run/cosmic-greeter/cosmic "/run/user/${cg_uid}" \
    /var/lib/cosmic-greeter/.config \
    /var/lib/cosmic-greeter/.local/state \
    /var/lib/cosmic-greeter/.local/share
  touch /var/lib/forge/cosmic-greeter-session.log
  chown -R cosmic-greeter:cosmic-greeter /run/cosmic-greeter /var/lib/cosmic-greeter
  chown cosmic-greeter:cosmic-greeter /var/lib/forge/cosmic-greeter-session.log
  chown cosmic-greeter:cosmic-greeter "/run/user/${cg_uid}" 2>/dev/null || true
  chmod 0755 /run/cosmic-greeter
  chmod 0700 "/run/user/${cg_uid}" 2>/dev/null || true
  chmod 0644 /var/lib/forge/cosmic-greeter-session.log
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

if getent group seat >/dev/null 2>&1 && getent passwd cosmic-greeter >/dev/null 2>&1; then
  usermod -aG seat cosmic-greeter 2>/dev/null || true
fi

if getent passwd sisyphus >/dev/null 2>&1; then
  for grp in video render input seat; do
    if getent group "$grp" >/dev/null 2>&1; then
      usermod -aG "$grp" sisyphus 2>/dev/null || true
    fi
  done
  for dev in /dev/dri/card* /dev/dri/renderD*; do
    [[ -e "$dev" ]] || continue
    if command -v setfacl >/dev/null 2>&1; then
      setfacl -m "u:sisyphus:rw" "$dev" 2>/dev/null || true
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