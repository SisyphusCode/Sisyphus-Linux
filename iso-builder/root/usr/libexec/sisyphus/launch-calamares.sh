#!/usr/bin/env bash
# Launch the Sisyphus native installer when running from Kiwi overlayroot live media.
set -euo pipefail

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
    echo "Sisyphus installer: overlay rootfsbase missing (not in overlayroot live mode?)" >&2
    exit 1
fi

if pgrep -x calamares >/dev/null 2>&1; then
    exit 0
fi

exec pkexec calamares -D