#!/usr/bin/env bash
# System bus for forge PID 1 — socket-activated (systemd dbus.socket model).
# Prefer classic dbus-daemon for reliable activation in early boot / clone.
set -euo pipefail

WRAPPER_LOG="${FORGE_LOG_DIR:-/var/log/forge}/dbus-wrapper.log"
mkdir -p /var/log/forge /run/dbus /var/lib/dbus
echo "=== $(date -Is 2>/dev/null || date) start-dbus ppid=$PPID pid=$$ LISTEN_PID=${LISTEN_PID:-unset} LISTEN_FDS=${LISTEN_FDS:-0} ===" >>"$WRAPPER_LOG"
echo "Open FDs in start-dbus.sh:" >>"$WRAPPER_LOG"
ls -l /proc/$$/fd >>"$WRAPPER_LOG" 2>&1 || true

# Ensure /etc/machine-id is initialized and not empty (dbus-broker fails without it)
if [[ ! -s /etc/machine-id ]]; then
  if command -v systemd-machine-id-setup >/dev/null 2>&1; then
    systemd-machine-id-setup 2>/dev/null || true
  fi
  if [[ ! -s /etc/machine-id ]] && command -v dbus-uuidgen >/dev/null 2>&1; then
    dbus-uuidgen --ensure=/etc/machine-id 2>/dev/null || true
  fi
  if [[ ! -s /etc/machine-id ]] && [[ -s /var/lib/dbus/machine-id ]]; then
    cp /var/lib/dbus/machine-id /etc/machine-id 2>/dev/null || true
  fi
  if [[ ! -s /etc/machine-id ]]; then
    echo "00000000000000000000000000000000" > /etc/machine-id 2>/dev/null || true
  fi
fi

# Ensure /var/lib/dbus/machine-id is also set up
if [[ ! -s /var/lib/dbus/machine-id ]]; then
  if [[ -s /etc/machine-id ]]; then
    mkdir -p /var/lib/dbus
    cp /etc/machine-id /var/lib/dbus/machine-id 2>/dev/null || true
  fi
fi

chown root:root /run/dbus 2>/dev/null || true
chmod 0755 /run/dbus 2>/dev/null || true
if command -v restorecon >/dev/null 2>&1; then
  restorecon -F /run/dbus 2>/dev/null || restorecon -R /run/dbus 2>/dev/null || true
fi
echo "label $(ls -Zd /run/dbus 2>/dev/null || echo unknown)" >>"$WRAPPER_LOG"

DBUS_DAEMON="/usr/bin/dbus-daemon"
DBUS_BROKER_LAUNCH="/usr/bin/dbus-broker-launch"

# Ensure LISTEN_PID is set to our current PID for systemd-style socket activation
export LISTEN_PID=$$

# Choose DBUS implementation - prefer broker like real RHEL10 systemd (dbus-broker package)
# Use mock daemon only in restricted ns where setpriv for 'dbus' user fails.
USE_MOCK_DAEMON=false
if command -v setpriv >/dev/null 2>&1; then
  if ! setpriv --reuid=dbus --regid=dbus --init-groups true 2>/dev/null; then
    USE_MOCK_DAEMON=true
  fi
fi

if [[ -f "$DBUS_BROKER_LAUNCH" && "$USE_MOCK_DAEMON" != "true" ]]; then
  # Standard / privileged: dbus-broker (preferred on RHEL)
  DBUS_BIN="$DBUS_BROKER_LAUNCH"
  ARGS=(--scope system --audit)
  echo "exec dbus-broker-launch args=${ARGS[*]}" >>"$WRAPPER_LOG"
  exec /usr/libexec/forge/exec-selinux-service.sh "$DBUS_BIN" "${ARGS[@]}"
elif [[ "$USE_MOCK_DAEMON" = "true" && -f "$DBUS_DAEMON" ]]; then
  # Restricted env (user ns etc): dbus-daemon with stripped user + permissive policy for testing
  DBUS_BIN="$DBUS_DAEMON"
  CONF="/usr/share/dbus-1/system.conf"
  [[ -f "$CONF" ]] || CONF="/etc/dbus-1/system.conf"
  
  MOCK_CONF="/run/dbus/system-root.conf"
  sed -e '/<user>/d' \
      -e 's|</busconfig>|<policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>|g' \
      "$CONF" > "$MOCK_CONF"
  CONF="$MOCK_CONF"
  
  if [[ -n "${LISTEN_FDS:-}" ]] && [[ "${LISTEN_FDS}" -gt 0 ]]; then
    echo "exec dbus-daemon --systemd-activation (LISTEN_FDS=${LISTEN_FDS}) [mock mode]" >>"$WRAPPER_LOG"
    exec /usr/libexec/forge/exec-selinux-service.sh "$DBUS_BIN" \
      --config-file="$CONF" --nofork --nopidfile --systemd-activation --address=systemd:
  else
    rm -f /run/dbus/system_bus_socket /run/dbus/pid
    ARGS=(--config-file="$CONF" --nofork --nopidfile)
    echo "exec dbus-daemon standalone (no LISTEN_FDS) [mock mode] args=${ARGS[*]}" >>"$WRAPPER_LOG"
    exec /usr/libexec/forge/exec-selinux-service.sh "$DBUS_BIN" "${ARGS[@]}"
  fi
elif [[ -f "$DBUS_DAEMON" ]]; then
  # Fallback to dbus-daemon (with user if possible)
  DBUS_BIN="$DBUS_DAEMON"
  CONF="/usr/share/dbus-1/system.conf"
  [[ -f "$CONF" ]] || CONF="/etc/dbus-1/system.conf"
  
  if [[ -n "${LISTEN_FDS:-}" ]] && [[ "${LISTEN_FDS}" -gt 0 ]]; then
    echo "exec dbus-daemon --systemd-activation (LISTEN_FDS=${LISTEN_FDS})" >>"$WRAPPER_LOG"
    exec /usr/libexec/forge/exec-selinux-service.sh --user=dbus "$DBUS_BIN" \
      --config-file="$CONF" --nofork --nopidfile --systemd-activation --address=systemd:
  else
    rm -f /run/dbus/system_bus_socket /run/dbus/pid
    ARGS=(--config-file="$CONF" --nofork --nopidfile)
    echo "exec dbus-daemon standalone (no LISTEN_FDS) args=${ARGS[*]}" >>"$WRAPPER_LOG"
    exec /usr/libexec/forge/exec-selinux-service.sh --user=dbus "$DBUS_BIN" "${ARGS[@]}"
  fi
else
  echo "ERROR: neither dbus-daemon nor dbus-broker-launch found!" >>"$WRAPPER_LOG"
  echo "ERROR: neither dbus-daemon nor dbus-broker-launch found!" >&2
  exit 127
fi
