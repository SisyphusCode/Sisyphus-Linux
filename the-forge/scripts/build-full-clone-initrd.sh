#!/usr/bin/env bash
# Build a *full* CIQ RLC Pro system clone initrd for QEMU using the host's dracut
# configuration + injected forge-core as the target init + current /etc/forge + scripts.
#
# This gives an almost byte-for-byte environment match for the real root filesystem
# (packages, GDM, gnome-shell, mutter, NetworkManager, plymouth, nvidia userspace,
#  full udev rules, fonts, schemas, LVM-visible content via snapshot drive, etc.)
# while letting us run our Rust forge-core as PID 1 safely in QEMU.
#
# The dracut-generated initrd still performs LVM? No - we point it at a raw snapshot
# of the already-assembled LV content. We force virtio drivers so the virtio disk
# provided by QEMU is usable for mounting the root.
#
# Usage:
#   ./scripts/build-full-clone-initrd.sh
#   FULL=1 DEBUG=1 ./scripts/run-qemu.sh
#
# Output: target/forge-full-clone-initrd.img
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TARGET="${ROOT}/target"

echo "=== Building release forge binaries for full clone ==="
cargo build --locked --release --manifest-path "$ROOT/Cargo.toml"

FORGE_CORE="$CARGO_TARGET/release/forge-core"
FORGE_LOGIND="$CARGO_TARGET/release/forge-logind"
FORGECTL="$CARGO_TARGET/release/forgectl"

[[ -x "$FORGE_CORE" ]] || { echo "forge-core not built"; exit 1; }

OUTIMG="${CARGO_TARGET}/forge-full-clone-initrd.img"
TMP_HOOK="/tmp/forge-pre-pivot-$$.sh"

# The hook runs inside the dracut initrd environment (before switch_root to real /).
# We copy the freshly built forge and current /etc/forge + scripts into the
# mounted root ($NEWROOT) so that when dracut does switch_root we exec our PID1
# against the *full* CIQ system image.
cat >"$TMP_HOOK" <<'HOOKSCRIPT'
#!/bin/sh
# dracut pre-pivot hook for Forge full-clone testing
[ -z "${NEWROOT:-}" ] && NEWROOT="/sysroot"

export PATH="/usr/sbin:/usr/bin:/sbin:/bin"

# Try to make the target root writable for injection (dracut often mounts ro from cmdline).
# With rw on cmdline + snapshot this should be a no-op or succeed.
mount -o remount,rw "${NEWROOT}" 2>/dev/null || true

# Ensure directories on real root (now that it may be rw)
mkdir -p "${NEWROOT}/usr/bin" "${NEWROOT}/sbin" \
         "${NEWROOT}/run/forge" "${NEWROOT}/var/log/forge" \
         "${NEWROOT}/run/dbus" "${NEWROOT}/run/udev" \
         "${NEWROOT}/etc/forge" "${NEWROOT}/usr/libexec/forge" || true

# Install forge-core binary (the one we included into this initrd)
if [ -x /usr/bin/forge-core ]; then
    cp -a /usr/bin/forge-core "${NEWROOT}/usr/bin/forge-core" 2>/dev/null || cp /usr/bin/forge-core "${NEWROOT}/usr/bin/forge-core" || echo "forge-clone: WARNING forge-core cp may have failed" > /dev/kmsg 2>/dev/null || true
    chmod 0755 "${NEWROOT}/usr/bin/forge-core" 2>/dev/null || true
    ln -sf ../usr/bin/forge-core "${NEWROOT}/sbin/init.forge" 2>/dev/null || true
    ln -sf /usr/bin/forge-core "${NEWROOT}/sbin/init" 2>/dev/null || true
fi

# Overlay the /etc/forge captured at initrd build time (user's exact current units).
# Use /. copy form to avoid name collisions like "forge/forge".
if [ -d /etc/forge ]; then
    if [ -d "${NEWROOT}/etc/forge" ] && [ ! -e "${NEWROOT}/etc/forge/.cloned-backup" ]; then
        mv "${NEWROOT}/etc/forge" "${NEWROOT}/etc/forge.real-backup" 2>/dev/null || true
    fi
    rm -rf "${NEWROOT}/etc/forge" 2>/dev/null || true
    mkdir -p "${NEWROOT}/etc/forge" 2>/dev/null || true
    cp -a /etc/forge/. "${NEWROOT}/etc/forge/" 2>/dev/null || cp -a /etc/forge/. "${NEWROOT}/etc/forge/" || true
    ( : > "${NEWROOT}/etc/forge/.cloned-from-initrd" ) 2>/dev/null || /bin/touch "${NEWROOT}/etc/forge/.cloned-from-initrd" 2>/dev/null || true
fi

# Overlay support scripts (start-*.sh, plymouth-*.sh etc) with current versions from build host.
if [ -d /usr/libexec/forge ]; then
    rm -rf "${NEWROOT}/usr/libexec/forge" 2>/dev/null || true
    mkdir -p "${NEWROOT}/usr/libexec/forge" 2>/dev/null || true
    cp -a /usr/libexec/forge/. "${NEWROOT}/usr/libexec/forge/" 2>/dev/null || cp -a /usr/libexec/forge/. "${NEWROOT}/usr/libexec/forge/" || true
    find "${NEWROOT}/usr/libexec/forge" -type f \( -name '*.sh' -o -name '*.py' \) -exec chmod 0755 {} + 2>/dev/null || /bin/chmod -R a+x "${NEWROOT}/usr/libexec/forge" 2>/dev/null || true
fi

# Log injection (will appear in dmesg / serial even if early logs are from dracut)
echo "forge-clone: injected forge + /etc/forge + /usr/libexec/forge into ${NEWROOT}" > /dev/kmsg 2>/dev/null || true

# machine-id for early dbus in the real root
if [ ! -s "${NEWROOT}/etc/machine-id" ] && [ -s /etc/machine-id ]; then
    cp /etc/machine-id "${NEWROOT}/etc/machine-id" 2>/dev/null || /bin/cp /etc/machine-id "${NEWROOT}/etc/machine-id" 2>/dev/null || true
fi

# Reset the persistent boot-attempts counter so the clone always gives forge
# a fresh chance (the real root snapshot inherits high count from prior host attempts).
mkdir -p "${NEWROOT}/var/lib/forge" "${NEWROOT}/run/forge" 2>/dev/null || true
echo 0 > "${NEWROOT}/var/lib/forge/boot-attempts" 2>/dev/null || /bin/echo 0 > "${NEWROOT}/var/lib/forge/boot-attempts" 2>/dev/null || true
echo 0 > "${NEWROOT}/run/forge/boot-attempts" 2>/dev/null || true
# Also mark a boot-ok early so post-recovery paths don't immediately schedule handoff.
: > "${NEWROOT}/run/forge/boot-ok" 2>/dev/null || /bin/touch "${NEWROOT}/run/forge/boot-ok" 2>/dev/null || true

exit 0
HOOKSCRIPT

chmod 0755 "$TMP_HOOK"

echo "=== Running dracut to produce full-clone initrd (this may take a minute) ==="
echo "Including:"
echo "  forge-core: $FORGE_CORE"
echo "  /etc/forge (host current)"
echo "  /usr/libexec/forge (host current + packaging/ciq)"
echo "  pre-pivot injection hook"
echo "  extra virtio drivers for QEMU virtio disk/net"

# Rebuild using host dracut config but force the QEMU drivers we need and inject artifacts.
# Using sudo because dracut needs to read host /boot, modules, and write the output.
sudo dracut \
    --force \
    --verbose \
    --add-drivers "virtio_blk virtio_pci virtio_net virtio_console virtio-rng" \
    --include "$FORGE_CORE" "/usr/bin/forge-core" \
    --include "$FORGECTL" "/usr/bin/forgectl" \
    --include "$FORGE_LOGIND" "/usr/bin/forge-logind" \
    --include "/etc/forge" "/etc/forge" \
    --include "/usr/libexec/forge" "/usr/libexec/forge" \
    --include "$TMP_HOOK" "/usr/lib/dracut/hooks/pre-pivot/99-forge-clone-inject.sh" \
    --kver "$(uname -r)" \
    "$OUTIMG"

rm -f "$TMP_HOOK"

if [[ -f "$OUTIMG" ]]; then
    echo
    echo "=== Full CIQ clone initrd ready ==="
    ls -lh "$OUTIMG"
    echo
    echo "To boot the full system clone with forge as init (recommended for diagnosis):"
    echo "  DEBUG=1 FULL=1 ./scripts/run-qemu.sh"
    echo "  (or CLONE=full ./scripts/run-qemu.sh)"
    echo
    echo "Inside the guest (via serial):"
    echo "  - Watch for ⏱️  WAVE logs and service status"
    echo "  - Failures will be printed by forge ghosttype_log + kmsg"
    echo "  - After boot (or hang) inspect /var/log/forge/* if it gets that far"
    echo
else
    echo "ERROR: dracut did not produce $OUTIMG" >&2
    exit 1
fi
