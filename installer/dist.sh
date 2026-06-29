#!/usr/bin/env bash
# Create Source0 tarball for sisyphus-installer-config SRPM builds.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-1.0.0}"
OUT="${2:-${ROOT}/installer/sisyphus-installer-config-${VERSION}.tar.gz}"

tar czf "${OUT}" -C "${ROOT}" installer/settings.conf installer/branding installer/modules
echo "Wrote ${OUT}"