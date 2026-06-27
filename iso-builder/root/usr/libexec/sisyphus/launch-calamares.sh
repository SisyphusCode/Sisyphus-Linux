#!/usr/bin/env bash
# Launch the Sisyphus native installer when running from Kiwi overlayroot live media.
set -euo pipefail

LOG=/var/log/forge/calamares.log
mkdir -p /var/log/forge

if [[ ! -f /etc/sisyphus/installer-enabled ]]; then
    exit 0
fi

overlay_rootfs_base() {
    if findmnt -rn -T /run/overlay/rootfsbase >/dev/null 2>&1; then
        echo /run/overlay/rootfsbase
        return 0
    fi
    if findmnt -rn -T /run/rootfsbase >/dev/null 2>&1; then
        echo /run/rootfsbase
        return 0
    fi
    return 1
}

if ! overlay_rootfs_base >/dev/null; then
    # Kiwi USB may expose squashfs+btrfs without /run/overlay/rootfsbase on early boot.
    if ! findmnt -rn -o FSTYPE / | grep -qE 'overlay|squashfs'; then
        echo "Sisyphus installer: overlay rootfsbase missing (not in overlayroot live mode?)" >&2
        exit 1
    fi
    echo "Sisyphus installer: proceeding without rootfsbase (squashfs/overlay live media)" >>"$LOG"
fi

if pgrep -x calamares >/dev/null 2>&1; then
    exit 0
fi

if ! getent passwd cosmic-greeter >/dev/null 2>&1; then
    echo "Sisyphus installer: cosmic-greeter user missing" >&2
    exit 1
fi

CG_UID="$(id -u cosmic-greeter)"
CG_RUNTIME="${XDG_RUNTIME_DIR:-/run/user/${CG_UID}}"
WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
CG_BUS="unix:path=${CG_RUNTIME}/bus"

# Detach from the forge service process tree — Calamares Qt refuses dead QML parents.
setsid runuser -u cosmic-greeter -- env \
    XDG_RUNTIME_DIR="$CG_RUNTIME" \
    WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
    DBUS_SESSION_BUS_ADDRESS="$CG_BUS" \
    XDG_SESSION_TYPE=wayland \
    GDK_BACKEND=wayland \
    QT_QPA_PLATFORM=wayland \
    calamares >>"$LOG" 2>&1 &

echo "=== $(date -Is 2>/dev/null || date) calamares spawned pid=$! uid=cosmic-greeter WAYLAND_DISPLAY=$WAYLAND_DISPLAY runtime=$CG_RUNTIME ===" >>"$LOG"
exit 0