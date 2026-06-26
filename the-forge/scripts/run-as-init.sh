#!/usr/bin/env bash
# Run forge-core as PID 1 inside a private PID namespace (requires privileges).
#
# WARNING: unsafe on a machine with an active GNOME session — can start logind/GDM
# against the real /dev/tty and log you out. Use ./scripts/forge-mock-boot.sh instead.
set -euo pipefail

if [[ -n "${XDG_SESSION_ID:-}" && "${FORGE_RUN_AS_INIT_CONFIRM:-}" != "1" ]]; then
  echo "Refusing to run on an active desktop session." >&2
  echo "Use: sudo FORGE_RUN_AS_INIT_CONFIRM=1 $0  (not recommended)" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${FORGE_BIN:-$ROOT/target/debug/forge-core}"

if [[ ! -x "$BIN" ]]; then
  echo "Building forge-core..."
  cargo build -p forge-core --manifest-path "$ROOT/Cargo.toml"
fi

export FORGE_UNIT_DIR="${FORGE_UNIT_DIR:-$ROOT/forge-core/examples/units}"
export FORGE_BOOT_SCRIPT="${FORGE_BOOT_SCRIPT:-$ROOT/forge-core/examples/boot.rhai}"
export FORGE_DEFAULT_TARGET="${FORGE_DEFAULT_TARGET:-$ROOT/forge-core/examples/default.target}"
export FORGE_LOG_DIR="${FORGE_LOG_DIR:-/tmp/forge/log}"
export FORGE_CONTROL_SOCKET="${FORGE_CONTROL_SOCKET:-/tmp/forge/control.sock}"

echo "Launching $BIN as PID 1 via unshare..."
echo "Control socket: $FORGE_CONTROL_SOCKET"
echo "Use: FORGE_CONTROL_SOCKET=$FORGE_CONTROL_SOCKET $ROOT/target/debug/forgectl status"
echo

exec unshare --fork --pid --mount-proc "$BIN"
