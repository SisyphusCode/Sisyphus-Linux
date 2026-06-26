#!/usr/bin/env bash
# Local sandbox boot against example units (no PID 1 required).
# SAFE: skips forge-early (pkill), GDM, getty, NetworkManager on a live desktop.
# For pre-reboot checks use: sudo ./scripts/forge-mock-boot.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${FORGE_NATIVE_MODE:-1}" == "1" || "${FORGE_NATIVE_MODE:-1}" == "true" ]]; then
  export FORGE_NATIVE_MODE=1
  export FORGE_UNIT_DIR="${FORGE_UNIT_DIR:-$ROOT/forge-core/examples/native-desktop}"
else
  export FORGE_UNIT_DIR="${FORGE_UNIT_DIR:-$ROOT/forge-core/examples/units}"
fi
export FORGE_BOOT_SCRIPT="${FORGE_BOOT_SCRIPT:-$ROOT/forge-core/examples/boot.rhai}"
export FORGE_DEFAULT_TARGET="${FORGE_DEFAULT_TARGET:-$ROOT/forge-core/examples/default.target}"
export FORGE_LOG_DIR="${FORGE_LOG_DIR:-/tmp/forge/log}"
export FORGE_JOURNAL="${FORGE_JOURNAL:-/tmp/forge/journal.jsonl}"
export FORGE_SYSTEMD_UNIT_DIR="${FORGE_SYSTEMD_UNIT_DIR:-$ROOT/forge-core/examples/systemd}"
export FORGE_CONTROL_SOCKET="${FORGE_CONTROL_SOCKET:-/tmp/forge/control.sock}"
export FORGE_DBUS_SOCKET="${FORGE_DBUS_SOCKET:-/tmp/forge/dbus.sock}"
export FORGE_UDEV_RULES="${FORGE_UDEV_RULES:-/tmp/forge/99-forge.rules}"
export FORGE_CGROUP_ROOT="${FORGE_CGROUP_ROOT:-/sys/fs/cgroup/forge.slice}"

echo "FORGE_NATIVE_MODE=${FORGE_NATIVE_MODE:-1}"
echo "FORGE_UNIT_DIR=$FORGE_UNIT_DIR"
echo "FORGE_BOOT_SCRIPT=$FORGE_BOOT_SCRIPT"
echo "FORGE_DEFAULT_TARGET=$FORGE_DEFAULT_TARGET"
echo "FORGE_LOG_DIR=$FORGE_LOG_DIR"
echo

mkdir -p "$FORGE_LOG_DIR" /tmp/forge "$FORGE_SYSTEMD_UNIT_DIR"

cargo run -p forge-core --bin forge-core --manifest-path "$ROOT/Cargo.toml"

echo
echo "Service logs:"
find "$FORGE_LOG_DIR" -type f -print -exec tail -n 3 {} \; 2>/dev/null || true
