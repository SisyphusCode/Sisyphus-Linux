#!/usr/bin/env bash
# Safe pre-reboot check for forge desktop boot — does NOT run forge-core or host logind.
#
# Previous versions started systemd-logind/elogind / forge-early (pkill) on the live system and
# kicked users out of GNOME. This script only validates install artifacts and exercises
# dbus + systemd1-stub on a private /run inside namespaces.
#
# Usage:
#   sudo ./scripts/forge-mock-boot.sh
#
# Full isolated VM test (optional, slow):
#   make qemu
set -euo pipefail

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  echo "forge-preflight: run as root (sudo $0)" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MOCK_ID="forge-preflight-$$"
MOCK_ROOT="/tmp/${MOCK_ID}"
RUN_ROOT="${MOCK_ROOT}/run"
DBUS_CONF="${MOCK_ROOT}/system.conf"
DBUS_PID=""
STUB_PID=""

host_pids() {
  pgrep -x 'systemd-logind\|elogind' 2>/dev/null | tr '\n' ' ' || true
}

HOST_LOGIND_BEFORE="$(host_pids)"
HOST_PID1="$(ps -o comm= -p 1)"

cleanup() {
  if [[ -n "$STUB_PID" ]] && kill -0 "$STUB_PID" 2>/dev/null; then
    kill -TERM "$STUB_PID" 2>/dev/null || true
  fi
  if [[ -n "$DBUS_PID" ]] && kill -0 "$DBUS_PID" 2>/dev/null; then
    kill -TERM "$DBUS_PID" 2>/dev/null || true
  fi
  umount "$RUN_ROOT" 2>/dev/null || true
  rm -rf "$MOCK_ROOT"
}
trap cleanup EXIT INT TERM

pass() { echo "OK   $1"; ok=$((ok + 1)); }
fail() { echo "FAIL $1"; fail_n=$((fail_n + 1)); }
ok=0
fail_n=0

echo "=== Forge preflight (live-session safe) ==="
echo "  host PID 1:     $HOST_PID1"
echo "  host logind:    ${HOST_LOGIND_BEFORE:-none}"
echo

echo "--- Install / unit checks ---"

[[ -x /usr/sbin/forge-core ]] && pass "forge-core installed" || fail "forge-core installed"
[[ -x /usr/libexec/forge/systemd1-stub.py ]] && pass "systemd1-stub installed" || fail "systemd1-stub installed"
[[ -x /usr/libexec/forge/systemd1-stub-activate.sh ]] && pass "systemd1-stub-activate installed" || fail "systemd1-stub-activate installed"
[[ -x /usr/libexec/forge/restorecon-forge.sh ]] && pass "restorecon-forge installed" || fail "restorecon-forge installed"
[[ -x /usr/libexec/forge/desktop-ready.sh ]] && pass "desktop-ready installed" || fail "desktop-ready installed"
[[ -x /usr/libexec/forge/gdm-greeter-setup.sh ]] && pass "gdm-greeter-setup installed" || fail "gdm-greeter-setup installed"
[[ -x /etc/gdm/Init/forge ]] && pass "GDM Init/forge hook" || fail "GDM Init/forge hook"
[[ -x /usr/libexec/forge/plymouth-forge-kill.sh ]] && pass "plymouth-forge-kill installed" || fail "plymouth-forge-kill installed"
[[ -x /usr/libexec/forge/pam-logind-create-session.py ]] && pass "pam-logind-create-session installed" || fail "pam-logind-create-session installed"
grep -q 'pam-forge-login-session' /etc/pam.d/login 2>/dev/null \
  && pass "login PAM forge hook" || fail "login PAM forge hook"
[[ -f /etc/X11/xorg.conf.d/10-forge-logind.conf ]] && pass "Xorg DontVTSwitch config" || fail "Xorg DontVTSwitch config"
[[ -x /usr/libexec/forge/start-forge-desktop.sh ]] && pass "start-forge-desktop installed" || fail "start-forge-desktop installed"
[[ -x /usr/libexec/forge/forge-desktop-session.py ]] && pass "forge-desktop-session installed" || fail "forge-desktop-session installed"
[[ -f /etc/forge/units/61-forge-desktop.forge.toml ]] && pass "forge-desktop unit" || fail "forge-desktop unit"
if grep -E '^\s*wants\s*=.*display-manager' /etc/forge/units/99-graphical.target.forge.toml >/dev/null 2>&1; then
  pass "graphical target wants display-manager (GDM enabled)"
elif grep -E '^\s*wants\s*=.*forge-desktop' /etc/forge/units/99-graphical.target.forge.toml >/dev/null 2>&1; then
  pass "graphical target wants forge-desktop (auto-login enabled)"
else
  fail "graphical target does not request display-manager or forge-desktop"
fi
[[ -f /etc/forge/desktop.toml ]] && pass "desktop.toml installed" || fail "desktop.toml installed"
[[ -x /usr/libexec/forge/start-logind.sh ]] && pass "start-logind installed" || fail "start-logind installed"
[[ -x /usr/libexec/forge/start-networkmanager.sh ]] && pass "start-networkmanager installed" || fail "start-networkmanager installed"
[[ -x /usr/libexec/forge/forge-early-mock.sh ]] && pass "forge-early-mock installed" || fail "forge-early-mock installed"
[[ -f /etc/forge/units/55-systemd1-stub.forge.toml ]] && pass "systemd1-stub unit" || fail "systemd1-stub unit"
grep -q 'systemd1-stub' /etc/forge/units/05-logind.forge.toml 2>/dev/null \
  && pass "logind ordered after systemd1-stub" || fail "logind ordered after systemd1-stub"
grep -q 'forge/systemd1-stub' /usr/share/dbus-1/system-services/org.freedesktop.systemd1.service 2>/dev/null \
  && pass "dbus org.freedesktop.systemd1 override (/usr/share)" \
  || fail "dbus org.freedesktop.systemd1 override (/usr/share — stock Exec=/bin/false wins over /etc)"
! grep -q '^Exec=/bin/false' /usr/share/dbus-1/system-services/org.freedesktop.systemd1.service 2>/dev/null \
  && pass "system dbus not using stock Exec=/bin/false" || fail "system dbus still Exec=/bin/false"
grep -q 'forge/systemd1-session-stub-wrapper' /usr/share/dbus-1/services/org.freedesktop.systemd1.service 2>/dev/null \
  && pass "session dbus org.freedesktop.systemd1 override" || fail "session dbus org.freedesktop.systemd1 override"
[[ -x /usr/libexec/forge/systemd1-session-stub.py ]] \
  && pass "systemd1-session-stub installed" || fail "systemd1-session-stub installed"
python3 -c "import dbus; from gi.repository import GLib" 2>/dev/null \
  && pass "python3-dbus + GObject" || fail "python3-dbus + GObject"

echo
echo "--- Isolated dbus + systemd1-stub (namespaces, private /run) ---"

mkdir -p "$RUN_ROOT/dbus" "$RUN_ROOT/systemd/seats" "$RUN_ROOT/systemd/sessions" "$RUN_ROOT/systemd/users"
mount -t tmpfs -o "size=64M" tmpfs "$RUN_ROOT"

cat >"$DBUS_CONF" <<'EOF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <listen>unix:path=/run/dbus/system_bus_socket</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF

SOCKET="$RUN_ROOT/dbus/system_bus_socket"
MACHINE_ID_FILE="$MOCK_ROOT/machine-id"
if [[ -s /var/lib/dbus/machine-id ]]; then
  cp /var/lib/dbus/machine-id "$MACHINE_ID_FILE"
else
  dbus-uuidgen --ensure="$MACHINE_ID_FILE" >/dev/null 2>&1 || echo "00000000000000000000000000000000" >"$MACHINE_ID_FILE"
fi

# Run dbus + stub in namespaces (synchronous — background unshare returns before the child finishes).
ISOLATED_RC=0
RUN_ROOT="$RUN_ROOT" DBUS_CONF="$DBUS_CONF" MACHINE_ID_FILE="$MACHINE_ID_FILE" \
  unshare --mount --pid --net --ipc --fork --kill-child bash -s <<'INNER' || ISOLATED_RC=$?
set -euo pipefail
mount --make-rprivate / 2>/dev/null || true
mkdir -p /run
mount --bind "$RUN_ROOT" /run
mkdir -p /run/dbus /run/systemd/seats /run/systemd/sessions /run/systemd/users
mkdir -p /var/lib/dbus
cp "$MACHINE_ID_FILE" /var/lib/dbus/machine-id
export DBUS_SYSTEM_BUS_ADDRESS="unix:path=/run/dbus/system_bus_socket"
export FORGE_DBUS_SYSTEM_BUS="$DBUS_SYSTEM_BUS_ADDRESS"

/usr/bin/dbus-daemon \
  --config-file="$DBUS_CONF" \
  --address="$DBUS_SYSTEM_BUS_ADDRESS" \
  --nopidfile --nofork &
DBUS_PID=$!
for _ in $(seq 1 100); do
  [[ -S /run/dbus/system_bus_socket ]] && break
  sleep 0.1
done
[[ -S /run/dbus/system_bus_socket ]] || { echo "dbus socket missing" >&2; exit 1; }

/usr/libexec/forge/systemd1-stub.py &
STUB_PID=$!
for _ in $(seq 1 100); do
  busctl --address="$DBUS_SYSTEM_BUS_ADDRESS" status org.freedesktop.systemd1 >/dev/null 2>&1 && break
  sleep 0.1
done
busctl --address="$DBUS_SYSTEM_BUS_ADDRESS" status org.freedesktop.systemd1 >/dev/null 2>&1 \
  || { echo "systemd1 stub not on bus" >&2; exit 1; }

DBUS_SYSTEM_BUS_ADDRESS="$DBUS_SYSTEM_BUS_ADDRESS" /usr/libexec/forge/systemd1-stub-activate.sh \
  || { echo "systemd1-stub-activate failed while stub already on bus" >&2; exit 1; }

busctl --address="$DBUS_SYSTEM_BUS_ADDRESS" call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager StartUnit ss "user@42.service" "replace" >/dev/null

SESSION_BUS="/run/user/42/bus"
mkdir -p /run/user/42
/usr/bin/dbus-daemon --session --address="unix:path=$SESSION_BUS" --nopidfile --nofork &
SESSION_DBUS_PID=$!
for _ in $(seq 1 50); do
  [[ -S "$SESSION_BUS" ]] && break
  sleep 0.1
done
[[ -S "$SESSION_BUS" ]] || { echo "session dbus socket missing" >&2; exit 1; }

DBUS_SESSION_BUS_ADDRESS="unix:path=$SESSION_BUS" \
  /usr/libexec/forge/systemd1-session-stub.py &
SESSION_STUB_PID=$!
for _ in $(seq 1 50); do
  busctl --address="unix:path=$SESSION_BUS" --user status org.freedesktop.systemd1 >/dev/null 2>&1 && break
  sleep 0.1
done
busctl --address="unix:path=$SESSION_BUS" --user status org.freedesktop.systemd1 >/dev/null 2>&1 \
  || { echo "session systemd1 stub not on bus" >&2; exit 1; }
ENV_OUT="$(busctl --address="unix:path=$SESSION_BUS" --user get-property org.freedesktop.systemd1 \
  /org/freedesktop/systemd1 org.freedesktop.systemd1.Manager Environment 2>/dev/null || true)"
echo "$ENV_OUT" | grep -q 'GBM_BACKEND=nvidia-drm' \
  || { echo "session Environment missing NVIDIA vars: $ENV_OUT" >&2; exit 1; }

cat > /run/systemd/seats/seat0 <<'SEAT'
# This is private data. Do not parse.
IS_SEAT0=1
CAN_MULTI_SESSION=1
CAN_TTY=1
CAN_GRAPHICAL=1
SEAT

touch /run/forge-preflight-ok
kill -TERM "$SESSION_STUB_PID" "$SESSION_DBUS_PID" "$STUB_PID" "$DBUS_PID" 2>/dev/null || true
wait "$SESSION_STUB_PID" "$SESSION_DBUS_PID" "$STUB_PID" "$DBUS_PID" 2>/dev/null || true
INNER

if [[ "$ISOLATED_RC" -eq 0 && -f "$RUN_ROOT/forge-preflight-ok" ]]; then
  pass "isolated dbus + systemd1-stub StartUnit(user@42.service)"
else
  fail "isolated dbus + systemd1-stub exercise (rc=$ISOLATED_RC)"
fi

if [[ -f "$RUN_ROOT/systemd/seats/seat0" ]] && grep -q 'CAN_TTY=1' "$RUN_ROOT/systemd/seats/seat0"; then
  pass "mock seat0 layout writable with CAN_TTY=1"
else
  fail "mock seat0 layout"
fi

echo
echo "--- Host safety (must be unchanged) ---"

if [[ "$(ps -o comm= -p 1)" == "$HOST_PID1" ]]; then
  pass "host PID 1 still $HOST_PID1"
else
  fail "host PID 1 changed"
fi

HOST_LOGIND_AFTER="$(host_pids)"
if [[ "$HOST_LOGIND_BEFORE" == "$HOST_LOGIND_AFTER" ]] && [[ -n "$HOST_LOGIND_AFTER" ]]; then
  pass "host logind pid unchanged ($HOST_LOGIND_AFTER)"
elif [[ -z "$HOST_LOGIND_BEFORE" && -z "$HOST_LOGIND_AFTER" ]]; then
  pass "host logind still absent"
elif pgrep -x 'systemd-logind\|elogind' >/dev/null 2>&1; then
  pass "host logind still running"
else
  fail "host logind died during preflight"
fi

if busctl status org.freedesktop.login1 >/dev/null 2>&1; then
  pass "host session bus / logind still healthy"
else
  fail "host logind not on system bus"
fi

echo
echo "Passed: $ok  Failed: $fail_n"

if [[ $fail_n -gt 0 ]]; then
  echo
  echo "Fix failures before rebooting with init=/usr/sbin/forge-core"
  exit 1
fi

echo
echo "Preflight passed. This does not start GDM — only a real reboot can confirm the greeter."
echo "If reboot still shows a black screen, from TTY3 (Ctrl+Alt+F3) run:"
echo "  ls -la /run/systemd/seats/seat0"
echo "  busctl status org.freedesktop.systemd1"
echo "  sudo tail -30 /var/log/forge/logind.log"
exit 0