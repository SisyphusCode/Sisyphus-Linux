#!/usr/bin/env bash
# Seed /etc/forge/runlevels from native unit runlevels fields (install-time).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UNITDIR="${1:-$ROOT/forge-core/examples/native-desktop}"
RUNLEVELS_ROOT="${FORGE_RUNLEVELS_DIR:-/etc/forge/runlevels}"

mkdir -p "$RUNLEVELS_ROOT"

count=0
for unit in "$UNITDIR"/*.service.toml; do
  [[ -f "$unit" ]] || continue
  name="$(grep -E '^name[[:space:]]*=' "$unit" | head -1 | sed 's/.*=\s*"\(.*\)".*/\1/')"
  [[ -n "$name" ]] || continue
  mapfile -t levels < <(grep -E '^runlevels[[:space:]]*=' "$unit" 2>/dev/null \
    | sed 's/.*=\s*\[\(.*\)\].*/\1/' \
    | tr ',' '\n' \
    | sed 's/["[:space:]]//g' \
    | grep -v '^$' || true)
  if [[ ${#levels[@]} -eq 0 ]]; then
    levels=(multi-user graphical)
  fi
  for rl in "${levels[@]}"; do
    mkdir -p "$RUNLEVELS_ROOT/$rl"
    : > "$RUNLEVELS_ROOT/$rl/$name"
    count=$((count + 1))
  done
done

echo 1 > "$RUNLEVELS_ROOT/.seeded"
echo "Seeded $count runlevel marker(s) from $UNITDIR"