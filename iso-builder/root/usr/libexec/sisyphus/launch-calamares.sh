#!/usr/bin/env bash
# Launch the Sisyphus native installer when running from Kiwi overlayroot live media.
set -euo pipefail

LOG=/var/log/forge/calamares.log
mkdir -p /var/log/forge

if [[ ! -f /etc/sisyphus/installer-enabled ]]; then
    exit 0
fi

find_unpack_source() {
    local root_opts lowerdirs first_lower

    for candidate in /run/overlay/rootfsbase /run/rootfsbase; do
        if findmnt -rn -T "${candidate}" >/dev/null 2>&1; then
            echo "${candidate}"
            return 0
        fi
    done

    root_opts="$(findmnt -rn -o OPTIONS / 2>/dev/null || true)"
    lowerdirs="$(printf '%s' "${root_opts}" | sed -n 's/.*lowerdir=\([^,]*\).*/\1/p')"
    first_lower="${lowerdirs%%:*}"
    if [[ -n "${first_lower}" && -d "${first_lower}" ]]; then
        echo "${first_lower}"
        return 0
    fi

    if [[ "$(findmnt -rn -o FSTYPE / 2>/dev/null || true)" == "squashfs" ]]; then
        echo "/"
        return 0
    fi

    return 1
}

normalize_unpack_source() {
    local source="${1}"
    if [[ -f "${source}/LiveOS/rootfs.img" ]]; then
        echo "${source}/LiveOS/rootfs.img"
        return 0
    fi
    if [[ -f "${source}/LiveOS/ext3fs.img" ]]; then
        echo "${source}/LiveOS/ext3fs.img"
        return 0
    fi
    echo "${source}"
}

write_unpackfs_conf() {
    local source="${1}" sourcefs=""
    if [[ -d "${source}" ]]; then
        cat > /etc/calamares/modules/unpackfs.conf <<EOF
---
unpack:
    - source: "${source}"
      destination: ""
EOF
        return 0
    fi

    sourcefs="$(blkid -o value -s TYPE "${source}" 2>/dev/null || true)"
    [[ -n "${sourcefs}" ]] || sourcefs="squashfs"

    cat > /etc/calamares/modules/unpackfs.conf <<EOF
---
unpack:
    - source: "${source}"
      sourcefs: "${sourcefs}"
      destination: ""
EOF
}

unpack_source="$(find_unpack_source 2>/dev/null || true)"
if [[ -z "${unpack_source}" ]]; then
    echo "Sisyphus installer: unable to determine unpackfs source from live media" >&2
    exit 1
fi

unpack_source="$(normalize_unpack_source "${unpack_source}")"
if [[ ! -e "${unpack_source}" ]]; then
    echo "Sisyphus installer: unpackfs source does not exist: ${unpack_source}" >&2
    exit 1
fi

write_unpackfs_conf "${unpack_source}"
echo "Sisyphus installer: using unpackfs source ${unpack_source}" >>"${LOG}"

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
setsid env \
    XDG_RUNTIME_DIR="$CG_RUNTIME" \
    WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
    DBUS_SESSION_BUS_ADDRESS="$CG_BUS" \
    XDG_SESSION_TYPE=wayland \
    GDK_BACKEND=wayland \
    QT_QPA_PLATFORM=wayland \
    calamares >>"$LOG" 2>&1 &

echo "=== $(date -Is 2>/dev/null || date) calamares spawned pid=$! uid=$(id -u) WAYLAND_DISPLAY=$WAYLAND_DISPLAY runtime=$CG_RUNTIME ===" >>"$LOG"
exit 0