#!/usr/bin/env bash
# Build a production-oriented initramfs embedding forge-core for QEMU/bare metal.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/target/forge-initramfs"
IMAGE="${ROOT}/target/forge-initramfs.cpio.gz"

echo "Building release binaries..."
cargo build --locked --release --manifest-path "$ROOT/Cargo.toml"

rm -rf "$OUT"
mkdir -p "$OUT"/{bin,sbin,etc/forge/{units,systemd},run/forge/log,run/user,var/log/forge,dev,proc,sys,run,tmp,usr/bin}

install -m 0755 "$ROOT/target/release/forge-core" "$OUT/sbin/init"
install -m 0755 "$ROOT/target/release/forge-logind" "$OUT/usr/bin/forge-logind"
install -m 0755 "$ROOT/target/release/forgectl" "$OUT/usr/bin/forgectl"

# Clone exact host /etc/forge for production-like units (graphical, dbus, gdm, waves, etc.)
if [ -d /etc/forge ]; then
  cp -a /etc/forge/. "$OUT/etc/forge/" 2>/dev/null || true
else
  install -m 0644 "$ROOT/forge-core/examples/default.target" "$OUT/etc/forge/default.target"
  install -m 0644 "$ROOT/forge-core/examples/boot.rhai" "$OUT/etc/forge/boot.rhai"
  install -m 0644 "$ROOT/forge-core/examples/network.toml" "$OUT/etc/forge/network.toml"
  cp -a "$ROOT/forge-core/examples/units/." "$OUT/etc/forge/units/"
  cp -a "$ROOT/forge-core/examples/systemd/." "$OUT/etc/forge/systemd/"
fi

# Clone installed forge support scripts from host for exact conditions
if [ -d /usr/libexec/forge ]; then
  mkdir -p "$OUT/usr/libexec/forge"
  cp -a /usr/libexec/forge/. "$OUT/usr/libexec/forge/" 2>/dev/null || true
fi

cat >"$OUT/etc/passwd" <<'EOF'
root:x:0:0:root:/root:/bin/sh
user:x:1000:1000::/home/user:/bin/sh
EOF
cat >"$OUT/etc/group" <<'EOF'
root:x:0:
user:x:1000:
EOF

# Minimal home for the test user
mkdir -p "$OUT/home/user"
chown -R 1000:1000 "$OUT/home/user" 2>/dev/null || true

# Very minimal dbus system configuration so dbus-daemon can start without a full package
mkdir -p "$OUT/usr/share/dbus-1/system.d" "$OUT/etc/dbus-1/system.d"
cat >"$OUT/etc/dbus-1/system.conf" <<'DBUSCONF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <limit name="max_incoming_bytes">100000000</limit>
  <limit name="max_outgoing_bytes">100000000</limit>
  <limit name="max_message_size">100000000</limit>
  <limit name="service_start_timeout">120000</limit>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
DBUSCONF

# Pull in a more complete userspace when available (helps desktop / real services).
# We prefer busybox for a small footprint, but also grab real tools if present.
if command -v busybox >/dev/null 2>&1; then
  BB="$(command -v busybox)"
  install -m 0755 "$BB" "$OUT/bin/busybox"
  chroot "$OUT" /bin/busybox --install -s /bin
fi

# Bundle host binaries into the initramfs (create parent dirs — plain install fails on nested paths).
install_bin() {
  local src="$1"
  [[ -x "$src" ]] || return 0
  local dest="$OUT$src"
  mkdir -p "$(dirname "$dest")"
  install -m 0755 "$src" "$dest"
}

# Real tools that help with desktop boot (dbus, udev, logins, network, graphics support, plymouth, wifi, gdm).
for candidate in \
  /usr/bin/dbus-daemon \
  /usr/bin/dbus-broker \
  /usr/bin/dbus-send \
  /usr/bin/busctl \
  /usr/lib/systemd/systemd-udevd \
  /usr/bin/udevadm \
  /usr/bin/udevd \
  /sbin/agetty \
  /usr/bin/agetty \
  /bin/getty \
  /sbin/getty \
  /usr/sbin/ip \
  /bin/ip \
  /usr/bin/ip \
  /usr/bin/bash \
  /bin/bash \
  /usr/bin/sh \
  /usr/bin/env \
  /usr/bin/gdm \
  /usr/sbin/gdm \
  /usr/libexec/gdm/gdm-session-worker \
  /usr/bin/Xorg \
  /usr/bin/plymouth \
  /usr/sbin/plymouthd \
  /usr/bin/NetworkManager \
  /usr/sbin/NetworkManager \
  /usr/sbin/wpa_supplicant \
  /usr/bin/nmcli \
  /usr/bin/gnome-shell \
  /usr/bin/mutter \
  /usr/bin/gnome-session ; do
  install_bin "$candidate"
done

# Bundle the full forge CIQ packaging scripts for realistic service startup (start-gdm, start-logind, etc.)
# This is key for GDM and desktop to work in the mock.
PKG_SRC="$ROOT/packaging/ciq"
if [[ -d "$PKG_SRC" ]]; then
  mkdir -p "$OUT/usr/libexec/forge"
  for f in "$PKG_SRC"/*.sh "$PKG_SRC"/*.py; do
    [[ -f "$f" ]] && install -m 0755 "$f" "$OUT/usr/libexec/forge/$(basename "$f")" || true
  done
  # Also the pam and other if needed
  if [[ -d "$ROOT/packaging/pam" ]]; then
    mkdir -p "$OUT/usr/share/forge" "$OUT/etc/pam.d"
    install -m 0644 "$ROOT/packaging/pam/"* "$OUT/usr/share/forge/" 2>/dev/null || true
  fi
fi

# Copy GDM data, configs, and support files for a more complete desktop mock.
if [ -x /usr/sbin/gdm ] || [ -x /usr/bin/gdm ]; then
  echo "Including GDM support files in initramfs..."
  # gdm binary already attempted above
  mkdir -p "$OUT/usr/share/gdm" "$OUT/etc/gdm" "$OUT/usr/libexec/gdm"
  cp -a /usr/share/gdm/* "$OUT/usr/share/gdm/" 2>/dev/null || true
  cp -a /etc/gdm/* "$OUT/etc/gdm/" 2>/dev/null || true
  cp -a /usr/libexec/gdm/* "$OUT/usr/libexec/gdm/" 2>/dev/null || true
  # gdm greeter and session files
  if [ -d /usr/share/gdm/greeter ]; then
    mkdir -p "$OUT/usr/share/gdm/greeter"
    cp -a /usr/share/gdm/greeter/* "$OUT/usr/share/gdm/greeter/" 2>/dev/null || true
  fi
fi

# Copy full gnome-shell and mutter private libs and dirs (for gnome-shell to load without "no gnome").
if [ -d /usr/lib64/gnome-shell ]; then
  mkdir -p "$OUT/usr/lib64/gnome-shell"
  cp -a /usr/lib64/gnome-shell/* "$OUT/usr/lib64/gnome-shell/" 2>/dev/null || true
fi
if [ -d /usr/lib64/mutter-8 ]; then
  mkdir -p "$OUT/usr/lib64/mutter-8"
  cp -a /usr/lib64/mutter-8/* "$OUT/usr/lib64/mutter-8/" 2>/dev/null || true
fi

# Recursively pull shared lib deps for key desktop/wifi/gdm bins to prevent "no wifi no gdm no gnome" errors in the clone.
for bin in /usr/bin/gnome-shell /usr/bin/mutter /usr/sbin/NetworkManager /usr/sbin/gdm /usr/bin/plymouth /usr/bin/gnome-session; do
  [ -x "$bin" ] || continue
  for lib in $(ldd "$bin" 2>/dev/null | awk '/=>/ {print $3}' | grep '^/' | sort -u); do
    if [ -f "$lib" ] && [ ! -f "$OUT$lib" ]; then
      mkdir -p "$(dirname "$OUT$lib")"
      cp -a "$lib" "$OUT$lib" 2>/dev/null || true
    fi
  done
done

# Ensure coreutils basics like mkdir (used by start-gdm.sh etc) are present; copy /usr/bin/mkdir and symlink to /bin if needed.
install_bin /usr/bin/mkdir
mkdir -p "$OUT/bin"
[ -x "$OUT/usr/bin/mkdir" ] && ln -sf /usr/bin/mkdir "$OUT/bin/mkdir" 2>/dev/null || true

# Copy Plymouth, NetworkManager, wifi configs for exact clone of host boot conditions (to debug hangs on plymouth/waves/wifi/gdm).
mkdir -p "$OUT/usr/share/plymouth" "$OUT/etc/plymouth" "$OUT/etc/NetworkManager" "$OUT/etc/wpa_supplicant"
cp -a /usr/share/plymouth/* "$OUT/usr/share/plymouth/" 2>/dev/null || true
cp -a /etc/plymouth/* "$OUT/etc/plymouth/" 2>/dev/null || true
cp -a /etc/NetworkManager/* "$OUT/etc/NetworkManager/" 2>/dev/null || true
cp -a /etc/wpa_supplicant/* "$OUT/etc/wpa_supplicant/" 2>/dev/null || true
# Host's /etc/forge already cloned above if present.

# Install dracut module bits if present (for realism)
if [[ -d "$ROOT/packaging/dracut/90forge" ]]; then
  mkdir -p "$OUT/usr/lib/dracut/modules.d/90forge"
  cp -a "$ROOT/packaging/dracut/90forge/"* "$OUT/usr/lib/dracut/modules.d/90forge/" 2>/dev/null || true
fi

# If we have agetty or getty but busybox didn't create the symlink, ensure common names exist.
for g in agetty getty; do
  if [[ -x "$OUT/sbin/$g" && ! -e "$OUT/bin/$g" ]]; then
    ln -sf "/sbin/$g" "$OUT/bin/$g" || true
  fi
done

if ! command -v busybox >/dev/null 2>&1; then
  for bin in /bin/sh /bin/cat /bin/true /bin/mount /bin/ip; do
    [[ -x "$bin" ]] && install -m 0755 "$bin" "$OUT$bin" || true
  done
fi

# Minimal device nodes if devtmpfs isn't ready early
mknod -m 622 "$OUT/dev/console" c 5 1 2>/dev/null || true
mknod -m 666 "$OUT/dev/null" c 1 3 2>/dev/null || true
mknod -m 666 "$OUT/dev/zero"  c 1 5 2>/dev/null || true
mknod -m 666 "$OUT/dev/tty"   c 5 0 2>/dev/null || true

# Bundle shared libraries required by the dynamically-linked forge-core and other copied binaries.
# Without ld.so and libc, /sbin/init fails with ENOENT (error -2) as seen in QEMU boots.
mkdir -p "$OUT/lib64"
for lib in /lib64/ld-linux-x86-64.so.2 /lib64/libc.so.6 /lib64/libgcc_s.so.1; do
  if [[ -f "$lib" ]]; then
    install -m 0755 "$lib" "$OUT$lib"
    # Also provide in /lib if some bins look there
    mkdir -p "$OUT/lib"
    ln -sf "/lib64/$(basename "$lib")" "$OUT/lib/$(basename "$lib")" 2>/dev/null || true
  fi
done

# Copy any other libs needed by copied tools (e.g. if dbus etc were installed)
shopt -s nullglob 2>/dev/null || true
for bin in "$OUT/sbin/init" "$OUT/usr/bin/"* "$OUT/bin/"* ; do
  [[ -x "$bin" ]] || continue
  for lib in $(ldd "$bin" 2>/dev/null | awk '/=>/ {print $3}' | grep -E '^/lib' | sort -u); do
    if [[ -f "$lib" && ! -f "$OUT$lib" ]]; then
      mkdir -p "$(dirname "$OUT$lib")"
      install -m 0755 "$lib" "$OUT$lib" 2>/dev/null || true
    fi
  done
done

# Runtime dirs expected by dbus, logind, DMs, DEs
mkdir -p "$OUT/run/dbus" "$OUT/run/lock" "$OUT/run/user" "$OUT/var/lib/dbus" "$OUT/var/tmp"
chmod 1777 "$OUT/tmp" "$OUT/var/tmp" 2>/dev/null || true

# dbus-daemon expects a machine-id very early; without it some builds stall or fail silently.
if command -v dbus-uuidgen >/dev/null 2>&1; then
  dbus-uuidgen --ensure="$OUT/var/lib/dbus/machine-id" >/dev/null 2>&1 || true
fi
if [[ ! -s "$OUT/var/lib/dbus/machine-id" ]]; then
  if [[ -s /etc/machine-id ]]; then
    cp /etc/machine-id "$OUT/var/lib/dbus/machine-id"
  elif command -v uuidgen >/dev/null 2>&1; then
    uuidgen >"$OUT/var/lib/dbus/machine-id"
  else
    echo "00000000000000000000000000000000" >"$OUT/var/lib/dbus/machine-id"
  fi
fi

# Minimal nsswitch + basic files so login/getty/DM don't complain
cat >"$OUT/etc/nsswitch.conf" <<'NSS'
passwd: files
group: files
shadow: files
NSS

# Very basic issue + hostname for console feel
echo "The Forge - Minimal Desktop-capable Initramfs" > "$OUT/etc/issue"
echo "forge" > "$OUT/etc/hostname"

# XDG_RUNTIME_DIR base (logind / sessions create per-user)
mkdir -p "$OUT/run/user/0" "$OUT/run/user/1000"
chmod 700 "$OUT/run/user/0" "$OUT/run/user/1000" 2>/dev/null || true

(
  cd "$OUT"
  find . -print0 | cpio --null -o --format=newc | gzip -9 >"$IMAGE"
)

echo "Initramfs written to $IMAGE"
echo "Launch: $ROOT/scripts/run-qemu.sh"
