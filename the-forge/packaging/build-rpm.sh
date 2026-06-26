#!/usr/bin/env bash
# Build the-forge RPM locally (Fedora / RHEL / Rocky).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RPMBUILD="${RPMBUILD:-$HOME/rpmbuild}"
OUTDIR="${OUTDIR:-$RPMBUILD}"

if ! command -v rpkg >/dev/null 2>&1; then
    echo "rpkg not found. Install with: sudo dnf install rpkg" >&2
    exit 1
fi

echo "==> Building RPM via rpkg (git archive + rpmbuild)..."
mkdir -p "$OUTDIR"
cd "$ROOT"
rpkg local --outdir "$OUTDIR" --spec packaging/the-forge.spec

echo "==> Done. RPMs are in $OUTDIR/x86_64/ and $OUTDIR/ (src.rpm)."