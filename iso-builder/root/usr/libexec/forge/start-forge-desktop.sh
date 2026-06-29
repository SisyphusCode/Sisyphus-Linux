#!/usr/bin/env bash
# Bypass GDM — auto-login GNOME via gdm-x-session + logind (same stack as systemd login).
set -euo pipefail

LOG=/var/log/forge/forge-desktop.log
mkdir -p /var/log/forge
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) start-forge-desktop ==="

desktop_user() {
  local u="Sisyphus"
  if [[ -f /etc/forge/desktop.toml ]]; then
    local line
    line="$(grep -E '^[[:space:]]*user[[:space:]]*=' /etc/forge/desktop.toml | head -1 || true)"
    if [[ -n "$line" ]]; then
      u="${line#*=}"
      u="${u#"${u%%[![:space:]]*}"}"
      u="${u%"${u##*[![:space:]]}"}"
      u="${u%\"}"
      u="${u#\"}"
      u="${u%\'}"
      u="${u#\'}"
    fi
  fi
  if [[ -n "${FORGE_DESKTOP_USER:-}" ]]; then
    u="$FORGE_DESKTOP_USER"
  fi
  echo "$u"
}

desktop_vt() {
  local vt="1"
  if [[ -f /etc/forge/desktop.toml ]]; then
    local line
    line="$(grep -E '^[[:space:]]*vt[[:space:]]*=' /etc/forge/desktop.toml | head -1 || true)"
    if [[ -n "$line" ]]; then
      vt="${line#*=}"
      vt="${vt// /}"
    fi
  fi
  echo "$vt"
}

ensure_xwrapper() {
  local f=/etc/X11/Xwrapper.config
  mkdir -p /etc/X11
  if [[ ! -f "$f" ]]; then
    cat >"$f" <<'EOF'
allowed_users=anybody
needs_root_rights=no
EOF
    return 0
  fi
  grep -q '^allowed_users=' "$f" 2>/dev/null \
    || echo 'allowed_users=anybody' >>"$f"
  grep -q '^needs_root_rights=' "$f" 2>/dev/null \
    || echo 'needs_root_rights=no' >>"$f"
}

USER_NAME="$(desktop_user)"
VT="$(desktop_vt)"
BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"

if ! id "$USER_NAME" >/dev/null 2>&1; then
  echo "start-forge-desktop: user '$USER_NAME' not found" >&2
  exit 1
fi

UID_NUM="$(id -u "$USER_NAME")"
RUNTIME="/run/user/${UID_NUM}"

ensure_xwrapper

for _ in $(seq 1 200); do
  [[ -S /run/dbus/system_bus_socket ]] && break
  sleep 0.1
done
for _ in $(seq 1 150); do
  busctl --address="$BUS" status org.freedesktop.DBus >/dev/null 2>&1 && break
  sleep 0.1
done
for _ in $(seq 1 150); do
  busctl --address="$BUS" status org.freedesktop.login1 >/dev/null 2>&1 && break
  sleep 0.1
done
for _ in $(seq 1 150); do
  busctl --address="$BUS" status org.freedesktop.systemd1 >/dev/null 2>&1 && break
  sleep 0.1
done

mkdir -p "$RUNTIME/gdm" /tmp/.X11-unix
chown -R "${USER_NAME}:${USER_NAME}" "$RUNTIME" 2>/dev/null || true
chmod 0700 "$RUNTIME" 2>/dev/null || true
chmod 1777 /tmp/.X11-unix 2>/dev/null || true

if [[ -x /usr/libexec/forge/restorecon-forge.sh ]]; then
  /usr/libexec/forge/restorecon-forge.sh || true
fi

busctl --address="$BUS" call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager StartUnit ss "user@${UID_NUM}.service" "replace" \
  >/dev/null 2>&1 || true

for _ in $(seq 1 150); do
  [[ -S "${RUNTIME}/bus" ]] && break
  sleep 0.1
done

if [[ ! -S "${RUNTIME}/bus" ]]; then
  echo "start-forge-desktop: session bus missing at ${RUNTIME}/bus" >&2
  exit 1
fi

if command -v setfacl >/dev/null 2>&1; then
  for vtdev in /dev/tty0 "/dev/tty${VT}" /dev/console; do
    [[ -e "$vtdev" ]] || continue
    setfacl -m "u:${USER_NAME}:rw,m::rw" "$vtdev" 2>/dev/null || true
  done
fi

if command -v chvt >/dev/null 2>&1; then
  chvt "$VT" 2>/dev/null || true
fi

if [[ ! -x /usr/libexec/forge/forge-desktop-session.py ]]; then
  echo "start-forge-desktop: forge-desktop-session.py not found" >&2
  exit 127
fi

echo "start-forge-desktop: launching ${USER_NAME} (uid ${UID_NUM}) on tty${VT}"

# Create logind x11 session (fork+leader) and exec gdm-x-session inside the helper.
exec env FORGE_DESKTOP_USER="$USER_NAME" DBUS_SYSTEM_BUS_ADDRESS="$BUS" \
  python3 /usr/libexec/forge/forge-desktop-session.py "$UID_NUM" "$USER_NAME" "$VT"