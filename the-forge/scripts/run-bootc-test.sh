#!/usr/bin/env bash
# Run the official CIQ RLC Pro bootc OCI image with Forge as PID 1.
# This is the "most realistic" clone: full shipped userspace, packages,
# bootc layout, exact GDM/NVIDIA/WiFi bits, etc.
#
# Usage:
#   ./scripts/run-bootc-test.sh
#   DEBUG=1 TIMEOUT=30s ./scripts/run-bootc-test.sh
#   INTERACTIVE=1 ./scripts/run-bootc-test.sh  # prompt before running
#
# Requirements:
#   - podman (rootless or root)
#   - You are logged in: podman login depot.ciq.com
#   - Built release: cargo build --release (done automatically)
#
# The container will use our forge-core as /sbin/init via bind mount.
# Mount your current /etc/forge and packaging scripts on top.
# Output (waves, logs, errors) goes to stdout + /tmp/forge-bootc-$$.log
#
# Inside the container you get a near-identical environment to a real
# bootc-installed RLC Pro system.
#
# Container safety notes:
# - No host /sys /proc /dev rw mounts (podman gives the container its own)
# - Private cgroups and network namespace (--network=none)
# - SELinux labels disabled to allow bind-mount overlays
# - Container-incompatible services (GDM, agetty, udev-trigger) are
#   gracefully skipped via FORGE_CONTAINER_TEST=1
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${IMAGE:-depot.ciq.com/rlc-9/rlc-9-oci-images/rlc-bootc:pro-9}"
TIMEOUT="${TIMEOUT:-45s}"
LOG="/tmp/forge-bootc-$$.log"

echo "=== Building release forge-core ==="
cargo build --locked --release --manifest-path "$ROOT/Cargo.toml"

FORGE_BIN="$ROOT/target/release/forge-core"
[[ -x "$FORGE_BIN" ]] || { echo "forge-core binary missing"; exit 1; }

echo "=== Using image: $IMAGE ==="
echo "Make sure you are logged in if the image is private:"
echo "  podman login depot.ciq.com"
echo "Pulling (if needed)..."
podman pull "$IMAGE" || true

echo "=== Container-isolated bootc test ==="
echo "This runs forge-core as PID 1 inside a container with:"
echo "  - Private cgroups, PID, and network namespaces"
echo "  - Container-incompatible services auto-skipped (FORGE_CONTAINER_TEST=1)"
echo "  - No host /sys /proc /dev mounts (podman provides its own)"
echo "Safer for iteration: ./scripts/mock-boot.sh or ./scripts/run-qemu.sh"

if [[ "${INTERACTIVE:-}" == "1" ]]; then
  read -t 5 -p "Press Enter to continue (auto-continuing in 5s)... " || true
  echo
fi

echo
echo "=== Running bootc container with Forge as PID 1 ==="
echo "Logs: $LOG"
echo "Use Ctrl-C to stop. Timeout: ${TIMEOUT}"
echo "Look for ⏱️ waves, LAUNCHING, FAILED, ONLINE, dbus, logind, NetworkManager."
echo

# Isolation:
# - --init=false: forge-core IS the init (PID 1), not podman's catatonit
# - --cgroupns=private: private cgroup namespace
# - --network=none: no network needed for init testing
# - tmpfs for /run and /tmp (normal early-boot expectations)
# - label=disable to allow overlaying our bits over the image
# - FORGE_CONTAINER_TEST=1: tells forge to skip container-incompatible services
#   (GDM, agetty/getty, udev-trigger — these need real hardware/TTY/graphics)
#
# NOTE: --userns=auto was removed because it prevents mount(2) syscalls
# inside the container. Forge's VFS mount code handles EBUSY gracefully
# (podman already provides /proc, /sys, /dev, /run).
podman run --rm \
  --init=false \
  --entrypoint /sbin/init \
  -v "$FORGE_BIN:/sbin/init:ro" \
  -v /etc/forge:/etc/forge:ro \
  -v "$ROOT/packaging/ciq:/usr/libexec/forge:ro" \
  -v "$ROOT/packaging/dbus:/usr/share/dbus-1/system-services:ro" \
  -v "$ROOT/packaging/dbus:/etc/dbus-1/system-services:ro" \
  -e FORGE_CONTAINER_TEST=1 \
  --cgroupns=private \
  --network=none \
  --tmpfs /run \
  --tmpfs /tmp \
  --security-opt=label=disable \
  --pids-limit=-1 \
  "$IMAGE" 2>&1 | tee "$LOG" &

RUN_PID=$!

echo "Letting container run for ${TIMEOUT}..."
timeout "${TIMEOUT}" tail -f "$LOG" 2>/dev/null || true

kill $RUN_PID 2>/dev/null || true
wait $RUN_PID 2>/dev/null || true

echo
echo "=== Bootc test finished. Full log: $LOG ==="

# Validate expected results
echo
echo "--- Result Validation ---"
PASS=0
FAIL=0

check_service() {
  local name="$1"
  local status="$2" # "ONLINE" or "ONESHOT" or "FAILED"
  local required="$3" # "required" or "optional"

  if grep -q "├── ${status}.*'${name}'" "$LOG" 2>/dev/null; then
    echo "  ✅ ${name}: ${status}"
    PASS=$((PASS + 1))
  elif [[ "$required" == "required" ]]; then
    echo "  ❌ ${name}: expected ${status} (REQUIRED)"
    FAIL=$((FAIL + 1))
  else
    echo "  ⚠️  ${name}: expected ${status} (optional, may be missing from image)"
  fi
}

# Core services that MUST work in a bootc container
check_service "dbus" "ONLINE" "required"
check_service "forge-early" "ONESHOT" "required"
check_service "plymouth-kill" "ONESHOT" "required"

# Infrastructure services — should work if present in image
check_service "polkit" "ONLINE" "optional"
check_service "NetworkManager" "ONLINE" "optional"
check_service "logind" "ONLINE" "optional"
check_service "udev" "ONLINE" "optional"

# Rust system bus services
if grep -q "Rust system bus services online" "$LOG" 2>/dev/null; then
  echo "  ✅ Rust D-Bus stubs (systemd1, hostname1, timedate1, locale1)"
  PASS=$((PASS + 1))
else
  echo "  ❌ Rust D-Bus stubs not registered"
  FAIL=$((FAIL + 1))
fi

# Boot should complete
if grep -q "Boot complete\|Target.*reached" "$LOG" 2>/dev/null; then
  echo "  ✅ Boot sequence completed"
  PASS=$((PASS + 1))
else
  echo "  ❌ Boot sequence did NOT complete"
  FAIL=$((FAIL + 1))
fi

# Expected container skips / known failures
echo
echo "--- Expected Container Skips ---"
for svc in display-manager getty-console getty-tty3 udev-trigger accounts-daemon; do
  if grep -q "FAILED.*'${svc}'" "$LOG" 2>/dev/null; then
    echo "  ℹ️  ${svc}: failed (expected — no hardware/TTY/display in container)"
  elif grep -q "SKIP.*'${svc}'\|container.*skip.*${svc}" "$LOG" 2>/dev/null; then
    echo "  ℹ️  ${svc}: skipped (FORGE_CONTAINER_TEST=1)"
  fi
done

echo
echo "--- Key Log Lines ---"
grep -E '⏱️.*(WAVE|LAUNCHING|ONESHOT|FAILED|READY|ONLINE|SKIP|DBUS.*Rust|BOOT|RECOVERY|STEADY|PROFILE)' "$LOG" | tail -60 || true

echo
echo "--- Summary ---"
echo "Passed: $PASS  Failed: $FAIL"
if [[ $FAIL -gt 0 ]]; then
  echo "❌ Some required services failed. Check the log: $LOG"
  exit 1
fi

echo "✅ Bootc container test passed."
echo
echo "Iterate by changing /etc/forge units or packaging/ciq/*.sh then re-running."
echo "The container sees the *exact* RLC Pro package set + your current config."