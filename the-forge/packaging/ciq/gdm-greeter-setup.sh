#!/usr/bin/env bash
# Prepare GDM greeter for forge PID 1 on hybrid NVIDIA laptops (e.g. ThinkPad P53).
set -euo pipefail

LOG=/var/log/forge/gdm-setup.log
mkdir -p /var/log/forge /run/gdm /run/forge
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) gdm-greeter-setup start ==="

touch /run/forge/plymouth-disabled 2>/dev/null || true

# Plymouth from initramfs holds KMS — kill before GDM claims the display.
for sig in TERM TERM KILL; do
  pkill "-$sig" plymouthd 2>/dev/null || true
  pkill "-$sig" plymouth 2>/dev/null || true
done
if command -v plymouth >/dev/null 2>&1; then
  plymouth quit 2>/dev/null || true
  plymouth deactivate 2>/dev/null || true
fi

# udev 61-gdm.rules disables Wayland on hybrid NVIDIA laptops — clear markers and override.
rm -f /run/udev/gdm-machine-is-laptop \
  /run/udev/gdm-machine-has-hybrid-graphics \
  /run/udev/gdm-machine-has-vendor-nvidia-driver 2>/dev/null || true

GDM_RUNTIME_CONFIG="/usr/libexec/gdm-runtime-config"
if [[ -x "$GDM_RUNTIME_CONFIG" ]]; then
  # Xorg greeter is more reliable under forge+logind than Wayland on P53 (Wayland hangs black).
  "$GDM_RUNTIME_CONFIG" set daemon WaylandEnable false 2>/dev/null || true
  "$GDM_RUNTIME_CONFIG" set daemon PreferredDisplayServer xorg 2>/dev/null || true
  "$GDM_RUNTIME_CONFIG" set daemon DefaultVT 1 2>/dev/null || true
fi

ENV_FILE=/run/gdm/forge-greeter-environment
cat >"$ENV_FILE" <<'EOF'
WLR_NO_HARDWARE_CURSORS=1
__GLX_VENDOR_LIBRARY_NAME=nvidia
GBM_BACKEND=nvidia-drm
EOF
chmod 0644 "$ENV_FILE" 2>/dev/null || true

if [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
  /usr/libexec/forge/release-graphics.sh || true
fi

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"
for _ in $(seq 1 150); do
  [[ -S /run/dbus/system_bus_socket ]] && break
  sleep 0.1
done
for _ in $(seq 1 100); do
  busctl --address="$BUS" status org.freedesktop.DBus >/dev/null 2>&1 && break
  sleep 0.1
done
for _ in $(seq 1 100); do
  busctl --address="$BUS" status org.freedesktop.login1 >/dev/null 2>&1 && break
  sleep 0.1
done

# GDM greeter Xorg talks to logind over dbus — socket must exist and be labeled.
if [[ -S /run/dbus/system_bus_socket ]] && command -v chcon >/dev/null 2>&1; then
  chcon -t system_dbusd_var_run_t /run/dbus/system_bus_socket 2>/dev/null || true
fi

if [[ -f /run/gdm/custom.conf ]]; then
  echo "runtime custom.conf:"
  cat /run/gdm/custom.conf || true
fi

echo "gdm-greeter-setup: finished"
exit 0