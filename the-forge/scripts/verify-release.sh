#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"

if [[ ! -d "$DIST" ]]; then
  echo "Run scripts/repro-build.sh first" >&2
  exit 1
fi

echo "Verifying release checksums..."
sha256sum -c "$DIST"/checksums/*.sha256

for bin in "$DIST/sbin/forge-core" "$DIST/bin/forgectl" "$DIST/bin/forge-logind"; do
  file "$bin"
  ldd "$bin" 2>/dev/null || true
done

echo "Running workspace tests..."
cargo test --locked --workspace --manifest-path "$ROOT/Cargo.toml"

echo "OK — release verification passed"
