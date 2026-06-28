#!/usr/bin/env bash
# Launch the Sisyphus native installer when running from Kiwi overlayroot live media.
set -euo pipefail

LOG=/var/lib/forge/calamares.log
mkdir -p "$(dirname "$LOG")"

if [[ ! -f /etc/sisyphus/installer-enabled ]]; then
    exit 0
fi

find_unpack_source() {
    local root_opts lowerdirs first_lower

    for candidate in /run/overlay/rootfsbase /run/rootfsbase; do
        [[ -e "${candidate}" ]] || continue
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
if [[ -n "${unpack_source}" ]]; then
    unpack_source="$(normalize_unpack_source "${unpack_source}")"
fi

if [[ -n "${unpack_source}" && -e "${unpack_source}" ]]; then
    write_unpackfs_conf "${unpack_source}"
    echo "Sisyphus installer: using unpackfs source ${unpack_source}" >>"${LOG}"
else
    # Keep packaged module default if detection fails in this environment.
    echo "Sisyphus installer: keeping default unpackfs.conf (detected='${unpack_source:-none}')" >>"${LOG}"
fi

if pgrep -x calamares >/dev/null 2>&1; then
    exit 0
fi

SESSION_RUNTIME="${XDG_RUNTIME_DIR:-}"
if [[ -z "${SESSION_RUNTIME}" || ! -d "${SESSION_RUNTIME}" ]]; then
    for socket in /run/user/*/wayland-*; do
        [[ -S "${socket}" ]] || continue
        SESSION_RUNTIME="$(dirname "${socket}")"
        WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-${socket##*-}}"
        break
    done
fi

if [[ -z "${SESSION_RUNTIME}" || ! -d "${SESSION_RUNTIME}" ]]; then
    echo "Sisyphus installer: no live Wayland runtime found" >&2
    exit 1
fi

WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
SESSION_BUS="unix:path=${SESSION_RUNTIME}/bus"

# Detach from the forge service process tree — Calamares Qt refuses dead QML parents.
setsid env \
    XDG_RUNTIME_DIR="$SESSION_RUNTIME" \
    WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
    DBUS_SESSION_BUS_ADDRESS="$SESSION_BUS" \
    XDG_SESSION_TYPE=wayland \
    XDG_CURRENT_DESKTOP=COSMIC \
    XDG_SESSION_DESKTOP=COSMIC \
    GDK_BACKEND=wayland \
    QT_QPA_PLATFORM=wayland \
    calamares >>"$LOG" 2>&1 &

echo "=== $(date -Is 2>/dev/null || date) calamares spawned pid=$! uid=$(id -u) WAYLAND_DISPLAY=$WAYLAND_DISPLAY runtime=$SESSION_RUNTIME ===" >>"$LOG"
exit 0