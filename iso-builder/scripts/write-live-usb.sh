#!/usr/bin/env bash
# Flash a KIWI .raw image to a USB disk.
set -euo pipefail

DISK="${DISK:-/dev/sda}"
IMAGE="${IMAGE:-}"
BS="${BS:-16M}"

pick_latest_image() {
    local newest=""
    newest="$(ls -1t /root/sisyphus-linux-build/*.raw /home/Sisyphus/*.raw 2>/dev/null | head -1 || true)"
    [[ -n "${newest}" ]] && echo "${newest}"
}

if [[ -z "${IMAGE}" ]]; then
    IMAGE="$(pick_latest_image || true)"
fi

if [[ -z "${IMAGE}" || ! -f "${IMAGE}" ]]; then
    echo "Set IMAGE=/path/to/sisyphus-linux*.raw (current: '${IMAGE:-unset}')" >&2
    exit 1
fi

if [[ ! -b "${DISK}" ]]; then
    echo "Block device not found: ${DISK}" >&2
    exit 1
fi

if [[ "${EUID}" -ne 0 ]]; then
    exec sudo --preserve-env=DISK,IMAGE,BS "$0"
fi

if [[ "$(lsblk -dn -o RM "${DISK}")" != "1" && "${FORCE:-0}" != "1" ]]; then
    echo "Refusing to write to non-removable disk ${DISK} (set FORCE=1 to override)." >&2
    exit 1
fi

echo "==> Flashing image"
echo "    Image: ${IMAGE}"
echo "    Disk:  ${DISK}"

while read -r part; do
    [[ "${part}" == "${DISK}" ]] && continue
    mp="$(findmnt -rn -o TARGET "${part}" 2>/dev/null || true)"
    if [[ -n "${mp}" ]]; then
        echo "Unmounting ${part} from ${mp}"
        umount "${mp}"
    fi
done < <(lsblk -ln -o PATH "${DISK}")

dd if="${IMAGE}" of="${DISK}" bs="${BS}" oflag=direct conv=fsync status=progress
sync
blockdev --rereadpt "${DISK}" || true
udevadm settle || true

echo "==> USB write complete"
lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT "${DISK}"
