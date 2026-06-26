#!/usr/bin/env bash
# Full isolated sandbox boot of forge-core.
#
# Runs a complete mock boot in private namespaces (mount + pid + etc.)
# so that:
#   * forge-core sees itself as PID 1 (inside the ns)
#   * /run is a private tmpfs (no host pollution)
#   * All waves, socket activation (LISTEN_FDS), service starts, dbus wait etc.
#     execute the real code paths.
#   * Errors/hangs seen in real boots (dbus listener, early units, etc.)
#     can be reproduced safely and quickly.
#
# Usage examples:
#   ./scripts/mock-boot.sh
#   FORGE_DEFAULT_TARGET=mock-desktop ./scripts/mock-boot.sh
#   FORGE_UNIT_DIR=forge-core/examples/units-mock ./scripts/mock-boot.sh
#   timeout 15s ./scripts/mock-boot.sh   # detect hangs
#
# Inside the ns you will see full "WAVE N", service launches, etc.
# Logs go to $FORGE_LOG_DIR (default /tmp/forge-mock-$$/log)
# and are also echoed.
#
# Requires unshare (util-linux) and usually root (or sufficient caps for --pid).
# Safer than bare run-as-init because we force private /run.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Configuration (override via env)
FORGE_BIN="${FORGE_BIN:-}"
if [[ -z "$FORGE_BIN" ]]; then
  if [[ -x "$ROOT/target/release/forge-core" ]]; then
    FORGE_BIN="$ROOT/target/release/forge-core"
  else
    echo "Building debug forge-core for mock..."
    cargo build -p forge-core --manifest-path "$ROOT/Cargo.toml"
    FORGE_BIN="$ROOT/target/debug/forge-core"
  fi
fi

export FORGE_MOCK_BOOT=1
if [[ "${FORGE_NATIVE_MODE:-1}" == "1" || "${FORGE_NATIVE_MODE:-1}" == "true" ]]; then
  export FORGE_NATIVE_MODE=1
  export FORGE_UNIT_DIR="${FORGE_UNIT_DIR:-$ROOT/forge-core/examples/native-desktop}"
else
  export FORGE_UNIT_DIR="${FORGE_UNIT_DIR:-$ROOT/forge-core/examples/units}"
fi
export FORGE_BOOT_SCRIPT="${FORGE_BOOT_SCRIPT:-$ROOT/forge-core/examples/boot.rhai}"

# Support both: FORGE_TARGET=<name> (preferred for simple name) or FORGE_DEFAULT_TARGET=<path-to-name-file>
if [[ -z "${FORGE_TARGET:-}" && -n "${FORGE_DEFAULT_TARGET:-}" && "$FORGE_DEFAULT_TARGET" != */* && "$FORGE_DEFAULT_TARGET" != *.* ]]; then
  export FORGE_TARGET="$FORGE_DEFAULT_TARGET"
fi
export FORGE_DEFAULT_TARGET="${FORGE_DEFAULT_TARGET:-$ROOT/forge-core/examples/default.target}"
if [[ -n "${FORGE_TARGET:-}" ]]; then
  export FORGE_TARGET
fi
MOCK_ID="forge-mock-$$"
MOCK_BASE="/tmp/${MOCK_ID}"
export FORGE_LOG_DIR="${FORGE_LOG_DIR:-${MOCK_BASE}/log}"
export FORGE_JOURNAL="${FORGE_JOURNAL:-${MOCK_BASE}/journal.jsonl}"
export FORGE_SYSTEMD_UNIT_DIR="${FORGE_SYSTEMD_UNIT_DIR:-$ROOT/forge-core/examples/systemd}"
export FORGE_CONTROL_SOCKET="${FORGE_CONTROL_SOCKET:-${MOCK_BASE}/control.sock}"
export FORGE_CGROUP_ROOT="${FORGE_CGROUP_ROOT:-/sys/fs/cgroup/forge.slice}"

echo "=== Forge full mock boot (isolated namespaces) ==="
echo "BIN:            $FORGE_BIN"
echo "NATIVE_MODE:    ${FORGE_NATIVE_MODE:-0}"
echo "UNITS:          $FORGE_UNIT_DIR"
echo "TARGET:         $FORGE_DEFAULT_TARGET"
echo "LOG_DIR:        $FORGE_LOG_DIR"
echo "MOCK_BASE:      $MOCK_BASE"
echo
echo "This runs forge-core as PID 1 *inside* the namespace with a private /run."
echo "Use Ctrl-C or timeout to stop. Full wave + service execution will be shown."
echo

mkdir -p "$FORGE_LOG_DIR" "$MOCK_BASE/run" "$MOCK_BASE" "$FORGE_SYSTEMD_UNIT_DIR"

# Prepare bind target dir for CIQ helper scripts. Regular user (even --map-root-user)
# cannot create directories under /usr as the DAC owner is real root.
# We tolerate leaving a root-owned dir behind; it only affects mock runs.
if [[ ! -d /usr/libexec/forge ]]; then
  mkdir -p /usr/libexec/forge 2>/dev/null || sudo mkdir -p /usr/libexec/forge 2>/dev/null || true
fi

# The inner script that sets up the private environment and execs forge.
# Use unquoted heredoc so $FORGE_BIN etc expand from outer shell.
INNER_SCRIPT=$(cat <<INNER
set -euo pipefail

# Private /run (this is the key for isolation and for replicating /run/dbus etc.)
mount -t tmpfs -o size=512M,mode=0755 tmpfs /run 2>/dev/null || true

# Essential dirs that real boot and units expect (private)
# NOTE: only pre-create dirs we control or that exist; for /var subdirs under protected
# paths we mount tmpfs on the *containing* existing dirs (/var/log, /var/lib) then mkdir inside.
mkdir -p \
  /run/dbus \
  /run/systemd/seats /run/systemd/sessions /run/systemd/users \
  /run/systemd/inhibit /run/systemd/ask-password /run/systemd/machines \
  /run/systemd/shutdown /run/systemd/resolve /run/systemd/journal \
  /run/forge /run/forge/log \
  /run/user /run/lock /run/gdm /run/log /run/udev \
  /tmp /var/tmp

# Make /tmp and /var/tmp private tmpfs too (common in early boot)
mount -t tmpfs -o size=256M,mode=1777 tmpfs /tmp 2>/dev/null || true
mount -t tmpfs -o size=128M,mode=1777 tmpfs /var/tmp 2>/dev/null || true

# Mount tmpfs over existing /var/log and /var/lib so that subdirs like /var/log/forge
# can be created inside as mapped-root (non-root outer user).
mkdir -p /var/log /var/lib 2>/dev/null || true
mount -t tmpfs -o size=64M,mode=0755 tmpfs /var/log 2>/dev/null || true
mount -t tmpfs -o size=16M,mode=0755 tmpfs /var/lib 2>/dev/null || true
mount --bind "$ROOT/packaging/ciq" /usr/libexec/forge 2>/dev/null || true

# Now create subdirs inside the private tmpfs mounts (these succeed)
mkdir -p /var/log/forge /var/lib/dbus /var/lib/forge /run/forge/log



# Minimal /dev nodes if not present (forge and some units create ttys)
for n in 0 1 2 3 console null; do
  case "\$n" in
    console) mknod -m 622 /dev/console c 5 1 2>/dev/null || true ;;
    null)    mknod -m 666 /dev/null    c 1 3 2>/dev/null || true ;;
    *)       mknod -m 622 "/dev/tty\$n" c 4 "\$n" 2>/dev/null || true ;;
  esac
done

# Propagate chosen log dir inside if it was on host-visible /tmp
export FORGE_LOG_DIR="${FORGE_LOG_DIR:-/run/forge/log}"
mkdir -p "\$FORGE_LOG_DIR"

# Make sure forge sees a reasonable PATH and DBUS address expectation
export PATH="${PATH:-/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin}"
if [[ -z "\${DBUS_SYSTEM_BUS_ADDRESS:-}" ]]; then
  export DBUS_SYSTEM_BUS_ADDRESS="unix:path=/run/dbus/system_bus_socket"
fi

echo "[mock-ns] /run is private tmpfs, pid ns active, FORGE_MOCK_BOOT=1"
echo "[mock-ns] Starting forge-core as init (pid 1 inside this ns) ..."
echo

# Exec forge. Because we are already the init of the pid namespace,
# forge will see process::id() == 1 and run the full (non-sandbox-skipped) paths.
exec "$FORGE_BIN"
INNER
)

# Launch in a fully isolated namespace.
# --mount-proc gives us a fresh /proc for the pid ns.
# We mount a private /run right after entering the ns.
# --map-root-user allows running without root (maps current user to root inside ns)
NS_FLAGS="--fork --pid --mount --uts --ipc --net --mount-proc --kill-child"
if unshare --help 2>&1 | grep -q map-root-user; then
  NS_FLAGS="--map-root-user $NS_FLAGS"
fi

unshare $NS_FLAGS \
  /bin/bash -c "$INNER_SCRIPT" \
  2>&1 | tee "${MOCK_BASE}/mock-boot.log" || true

echo
echo "=== Mock boot finished (or timed out / interrupted) ==="
echo "Full log: ${MOCK_BASE}/mock-boot.log"
echo "Service logs:"
find "$FORGE_LOG_DIR" -type f -print -exec tail -n 5 {} \; 2>/dev/null || true
echo
echo "Journal (if any): $FORGE_JOURNAL"
echo
echo "To inspect a particular run:"
echo "  less ${MOCK_BASE}/mock-boot.log"
echo "  ls -l $FORGE_LOG_DIR"
echo "  tail -100 $FORGE_JOURNAL"
echo
echo "Tip: set FORGE_DEFAULT_TARGET=mock-desktop or FORGE_UNIT_DIR=.../units-mock for lighter runs."
echo "     Add 'timeout 20s' in front to catch hangs."
