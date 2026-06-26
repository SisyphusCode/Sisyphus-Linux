#!/usr/bin/env bash
# Reproducible release build — same flags everywhere, locked deps, checksum manifest.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
VERSION="$(cat "$ROOT/VERSION")"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}"

echo "The Forge v${VERSION} reproducible build"
echo "SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH}"

cd "$ROOT"
cargo clean
cargo build --locked --release --workspace

rm -rf "$DIST"
mkdir -p "$DIST/bin" "$DIST/sbin" "$DIST/checksums"

install -m 0755 "$ROOT/target/release/forge-core" "$DIST/sbin/forge-core"
install -m 0755 "$ROOT/target/release/forgectl" "$DIST/bin/forgectl"
install -m 0755 "$ROOT/target/release/forge-logind" "$DIST/bin/forge-logind"

(
  cd "$DIST"
  sha256sum sbin/forge-core bin/forgectl bin/forge-logind > "checksums/the-forge-${VERSION}.sha256"
)

cat >"$DIST/BUILDINFO" <<EOF
name=the-forge
version=${VERSION}
source_date_epoch=${SOURCE_DATE_EPOCH}
rustc=$(rustc --version)
target=$(rustc -vV | awk '/host/ {print $2}')
profile=release
flags=--locked
EOF

echo "Release artifacts in $DIST"
cat "$DIST/checksums/the-forge-${VERSION}.sha256"
