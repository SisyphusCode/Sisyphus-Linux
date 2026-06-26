#!/usr/bin/env bash
# Build a Sisyphus Linux OEM disk image with Kiwi.
#
# Requirements (Fedora host):
#   sudo dnf install python3-kiwi kiwi-tools xorriso grub2-efi-x64 shim-x64
#
# Usage:
#   ./build.sh
#   TARGET_DIR=/var/tmp/sisyphus-out ./build.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="${TARGET_DIR:-/root/sisyphus-linux-build}"
KIWI="${KIWI:-kiwi-ng-3}"

if [[ "${EUID}" -ne 0 ]]; then
    echo "Run as root: sudo $0" >&2
    exit 1
fi

# Kiwi chroot RPM needs permissive SELinux on enforcing hosts.
if [[ "$(getenforce 2>/dev/null)" == "Enforcing" ]]; then
    echo "==> Temporarily setting SELinux permissive for image build"
    setenforce 0
fi

# Fedora 45 / RPM 6 enforces %_pkgverify_level=all globally; Kiwi bootstrap
# uses host dnf --installroot and gpgcheck=0 in config.xml is not enough.
if [[ "$(rpm --eval '%{_pkgverify_level}' 2>/dev/null)" == "all" ]]; then
    echo "==> Relaxing RPM verify level for Kiwi bootstrap (digest)"
    echo '%_pkgverify_level digest' > /etc/rpm/macros.verify
fi

if ! command -v "${KIWI}" >/dev/null 2>&1; then
    echo "${KIWI} not found. Install: dnf install python3-kiwi" >&2
    exit 1
fi

mkdir -p "${TARGET_DIR}"

echo "==> Building Sisyphus Linux OEM image"
echo "    Description: ${ROOT}"
echo "    Output:      ${TARGET_DIR}"

"${KIWI}" system build \
    --description "${ROOT}" \
    --target-dir "${TARGET_DIR}" \
    --allow-existing-root

echo "==> Done."
echo "    Image artifacts: ${TARGET_DIR}"
ls -la "${TARGET_DIR}"/*.raw "${TARGET_DIR}"/*.qcow2 2>/dev/null || ls -la "${TARGET_DIR}/"