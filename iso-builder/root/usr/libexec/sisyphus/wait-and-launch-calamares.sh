#!/usr/bin/env bash
# Wait for overlayroot + Wayland, then start Calamares on live installer media.
set -euo pipefail

[[ -f /etc/sisyphus/installer-enabled ]] || exit 0

for _ in $(seq 1 120); do
    if findmnt -rn -T /run/overlay/rootfsbase >/dev/null 2>&1 \
        || findmnt -rn -T /run/rootfsbase >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

for _ in $(seq 1 180); do
    for socket in /run/user/*/wayland-*; do
        [[ -S "${socket}" ]] || continue
        export XDG_RUNTIME_DIR
        XDG_RUNTIME_DIR="$(dirname "$(dirname "${socket}")")"
        export WAYLAND_DISPLAY="wayland-${socket##*-}"
        exec /usr/libexec/sisyphus/launch-calamares.sh
    done
    sleep 1
done

echo "Sisyphus installer: no Wayland display found" >&2
exit 1