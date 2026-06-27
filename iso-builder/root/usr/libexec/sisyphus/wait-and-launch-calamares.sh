#!/usr/bin/env bash
# Wait for overlayroot + Wayland, then start Calamares on live installer media.
set -euo pipefail

LOG=/var/log/forge/sisyphus-installer.log
mkdir -p /var/log/forge

# Never block graphical.target boot — Wayland may take a minute to appear.
if [[ "${SISYPHUS_INSTALLER_BG:-}" != 1 ]]; then
    export SISYPHUS_INSTALLER_BG=1
    setsid bash -c 'exec >>"'"$LOG"'" 2>&1; SISYPHUS_INSTALLER_BG=1 exec "$0"' \
        /usr/libexec/sisyphus/wait-and-launch-calamares.sh </dev/null &
    echo "=== $(date -Is 2>/dev/null || date) spawned background calamares waiter pid=$! ===" >>"$LOG"
    exit 0
fi

exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) wait-and-launch-calamares pid=$$ ==="

[[ -f /etc/sisyphus/installer-enabled ]] || exit 0

for _ in $(seq 1 30); do
    if findmnt -rn -T /run/overlay/rootfsbase >/dev/null 2>&1 \
        || findmnt -rn -T /run/rootfsbase >/dev/null 2>&1 \
        || findmnt -rn -o FSTYPE / | grep -qE 'overlay|squashfs'; then
        break
    fi
    sleep 0.2
done

for i in $(seq 1 600); do
    for socket in /run/user/*/wayland-*; do
        [[ -S "${socket}" ]] || continue
        export XDG_RUNTIME_DIR
        XDG_RUNTIME_DIR="$(dirname "${socket}")"
        export WAYLAND_DISPLAY="wayland-${socket##*-}"
        echo "Wayland socket found: $socket (XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR WAYLAND_DISPLAY=$WAYLAND_DISPLAY)"
        sleep 3
        /usr/libexec/sisyphus/launch-calamares.sh
        exit 0
    done
    if (( i % 30 == 0 )); then
        echo "still waiting for Wayland display (attempt $i/600)..."
        ls -la /run/user/*/ 2>/dev/null | head -20 || true
    fi
    sleep 1
done

echo "Sisyphus installer: no Wayland display found after 600s" >&2
exit 1