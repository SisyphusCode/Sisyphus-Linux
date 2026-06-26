#!/usr/bin/env bash
# Install forge-core and forgectl onto the target root (default: /).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESTDIR="${1:-/}"
DESTDIR="${DESTDIR%/}"
[[ -z "$DESTDIR" ]] && DESTDIR="/"

if [[ "$DESTDIR" == "/" ]]; then
  PREFIX="/usr"
else
  PREFIX="$DESTDIR/usr"
fi
SBINDIR="${PREFIX}/sbin"
BINDIR="${PREFIX}/bin"
LIBEXECDIR="${PREFIX}/libexec/forge"
UNITDIR="${DESTDIR}/etc/forge/units"
SYSCONFDIR="${DESTDIR}/etc/forge"
INSTALL_UNITS="${FORGE_INSTALL_UNITS:-replace}"
INSTALL_NATIVE="${FORGE_NATIVE_MODE:-1}"
if [[ "$INSTALL_NATIVE" == "1" || "$INSTALL_NATIVE" == "true" ]]; then
  UNIT_SOURCE="$ROOT/forge-core/examples/native-desktop"
  UNIT_KIND="native OpenRC-style"
else
  UNIT_SOURCE="$ROOT/forge-core/examples/units"
  UNIT_KIND="forge.toml (systemd-compat)"
fi

resolve_cargo() {
    if [[ -n "${CARGO:-}" && -x "${CARGO}" ]]; then
        echo "${CARGO}"
        return 0
    fi
    if command -v cargo >/dev/null 2>&1; then
        command -v cargo
        return 0
    fi
    local user="${SUDO_USER:-${USER:-}}"
    if [[ -n "$user" && -x "/home/$user/.cargo/bin/cargo" ]]; then
        echo "/home/$user/.cargo/bin/cargo"
        return 0
    fi
    return 1
}

# Check whether we already have usable release binaries.
have_release_binaries() {
    for bin in forge-core forgectl forge-logind forge-journalctl; do
        [[ -x "$ROOT/target/release/$bin" ]] || return 1
    done
    return 0
}

if [[ "${FORGE_SKIP_BUILD:-}" == "1" ]]; then
    echo "Skipping build (FORGE_SKIP_BUILD=1)"
elif have_release_binaries; then
    echo "Using existing release binaries from target/release/ (set FORGE_SKIP_BUILD=0 to force rebuild)"
elif CARGO_BIN="$(resolve_cargo)"; then
    echo "Building release binaries with $CARGO_BIN..."
    user="${SUDO_USER:-${USER:-}}"
    if [[ -n "$user" && "$CARGO_BIN" == "/home/$user/.cargo/bin/cargo" ]]; then
        # Running under sudo with a rustup install in the user's home.
        # Cargo finds rustc via PATH + rustup shims/toolchains, so we must inject the user's env.
        # We run the build as root (to write into the tree) but with a PATH that lets rustup locate rustc.
        env \
            PATH="/home/$user/.cargo/bin:${PATH:-/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin}" \
            CARGO_HOME="${CARGO_HOME:-/home/$user/.cargo}" \
            RUSTUP_HOME="${RUSTUP_HOME:-/home/$user/.rustup}" \
            "$CARGO_BIN" build --locked --release --manifest-path "$ROOT/Cargo.toml"
    else
        "$CARGO_BIN" build --locked --release --manifest-path "$ROOT/Cargo.toml"
    fi
else
    echo "cargo not found in PATH or ~/.cargo/bin (set CARGO=... or FORGE_SKIP_BUILD=1)" >&2
    exit 127
fi

for bin in forge-core forgectl forge-logind forge-journalctl; do
    if [[ ! -x "$ROOT/target/release/$bin" ]]; then
        echo "Missing $ROOT/target/release/$bin — build first (cargo build --release) or use FORGE_SKIP_BUILD=1 if binaries exist" >&2
        exit 1
    fi
done

install -d "$SBINDIR" "$BINDIR" "${DESTDIR}/usr/libexec" "$LIBEXECDIR" "$UNITDIR" "$SYSCONFDIR" "${DESTDIR}/etc/forge/systemd" "${DESTDIR}/usr/lib/forge/systemd"
install -m 0755 "$ROOT/target/release/forge-core" "$SBINDIR/forge-core"
install -m 0755 "$ROOT/target/release/forgectl" "$BINDIR/forgectl"
install -m 0755 "$ROOT/target/release/forge-logind" "$BINDIR/forge-logind"
install -m 0755 "$ROOT/target/release/forge-journalctl" "$BINDIR/forge-journalctl"
install -m 0755 "$ROOT/packaging/ciq/setup-net.sh" "$LIBEXECDIR/setup-net.sh"
install -m 0755 "$ROOT/packaging/ciq/start-gdm.sh" "$LIBEXECDIR/start-gdm.sh"
install -m 0755 "$ROOT/packaging/ciq/start-forge-desktop.sh" "$LIBEXECDIR/start-forge-desktop.sh"
install -m 0755 "$ROOT/packaging/ciq/forge-desktop-session.py" "$LIBEXECDIR/forge-desktop-session.py"
install -m 0755 "$ROOT/packaging/ciq/release-graphics.sh" "$LIBEXECDIR/release-graphics.sh"
install -m 0755 "$ROOT/packaging/ciq/restorecon-forge.sh" "$LIBEXECDIR/restorecon-forge.sh"
install -m 0755 "$ROOT/packaging/ciq/forge-run-layout.sh" "$LIBEXECDIR/forge-run-layout.sh"
install -m 0755 "$ROOT/packaging/ciq/desktop-ready.sh" "$LIBEXECDIR/desktop-ready.sh"
install -m 0755 "$ROOT/packaging/ciq/gdm-greeter-setup.sh" "$LIBEXECDIR/gdm-greeter-setup.sh"
install -m 0755 "$ROOT/packaging/ciq/plymouth-forge-kill.sh" "$LIBEXECDIR/plymouth-forge-kill.sh"
install -m 0755 "$ROOT/packaging/ciq/relabel-watch.sh" "$LIBEXECDIR/relabel-watch.sh"
install -m 0755 "$ROOT/packaging/ciq/gdm-init-forge" "${DESTDIR}/etc/gdm/Init/forge"
install -d "${DESTDIR}/etc/X11/xorg.conf.d"
install -m 0644 "$ROOT/packaging/xorg/10-forge-logind.conf" \
  "${DESTDIR}/etc/X11/xorg.conf.d/10-forge-logind.conf"
if [[ ! -f "${DESTDIR}/etc/X11/Xwrapper.config" ]]; then
  install -d "${DESTDIR}/etc/X11"
  install -m 0644 "$ROOT/packaging/xorg/Xwrapper-forge.conf" \
    "${DESTDIR}/etc/X11/Xwrapper.config"
fi
install -m 0755 "$ROOT/packaging/ciq/nm-relabel-resolv.sh" "$LIBEXECDIR/nm-relabel-resolv.sh"
install -d "${DESTDIR}/etc/NetworkManager/dispatcher.d"
install -m 0755 "$ROOT/packaging/ciq/nm-relabel-resolv.sh" \
  "${DESTDIR}/etc/NetworkManager/dispatcher.d/99-forge-relabel-resolv.sh"
install -m 0755 "$ROOT/packaging/ciq/start-logind.sh" "$LIBEXECDIR/start-logind.sh"
install -m 0755 "$ROOT/packaging/ciq/start-networkmanager.sh" "$LIBEXECDIR/start-networkmanager.sh"
install -d "${DESTDIR}/etc/gdm/custom.conf.d"
install -m 0644 "$ROOT/packaging/ciq/gdm-forge.conf" "${DESTDIR}/etc/gdm/custom.conf.d/forge.conf"
install -m 0755 "$ROOT/packaging/ciq/exec-selinux-service.sh" "$LIBEXECDIR/exec-selinux-service.sh"
install -m 0755 "$ROOT/packaging/ciq/start-dbus.sh" "$LIBEXECDIR/start-dbus.sh"
install -m 0755 "$ROOT/scripts/verify-install.sh" "${DESTDIR}/usr/bin/forge-verify-install"
install -m 0755 "$ROOT/scripts/forge-boot-enable.sh" "${DESTDIR}/usr/sbin/forge-boot-enable"
install -m 0755 "$ROOT/scripts/forge-boot-disable.sh" "${DESTDIR}/usr/sbin/forge-boot-disable"
install -m 0755 "$ROOT/packaging/ciq/start-udevd.sh" "$LIBEXECDIR/start-udevd.sh"
install -m 0755 "$ROOT/packaging/ciq/start-agetty.sh" "$LIBEXECDIR/start-agetty.sh"
install -m 0755 "$ROOT/packaging/ciq/start-polkit.sh" "$LIBEXECDIR/start-polkit.sh"
install -m 0755 "$ROOT/packaging/ciq/start-accounts-daemon.sh" "$LIBEXECDIR/start-accounts-daemon.sh"
install -m 0755 "$ROOT/packaging/ciq/forge-early.sh" "$LIBEXECDIR/forge-early.sh"
install -m 0755 "$ROOT/packaging/ciq/forge-early-mock.sh" "$LIBEXECDIR/forge-early-mock.sh"
install -m 0755 "$ROOT/packaging/ciq/systemd1-stub.py" "$LIBEXECDIR/systemd1-stub.py"
install -m 0755 "$ROOT/packaging/ciq/systemd1-stub-wrapper.sh" "$LIBEXECDIR/systemd1-stub-wrapper.sh"
install -m 0755 "$ROOT/packaging/ciq/systemd1-stub-activate.sh" "$LIBEXECDIR/systemd1-stub-activate.sh"
install -m 0755 "$ROOT/packaging/ciq/systemd1-session-stub.py" "$LIBEXECDIR/systemd1-session-stub.py"
install -m 0755 "$ROOT/packaging/ciq/systemd1-session-stub-wrapper.sh" "$LIBEXECDIR/systemd1-session-stub-wrapper.sh"

DBUS_SVC_DIR="${DESTDIR}/etc/dbus-1/system-services"
DBUS_SYSTEM_SVC_DIR="${DESTDIR}/usr/share/dbus-1/system-services"
DBUS_SESSION_SVC_DIR="${DESTDIR}/usr/share/dbus-1/services"
FORGE_BACKUP_DIR="${DESTDIR}/etc/forge/backup"
install -d "$DBUS_SVC_DIR" "$DBUS_SYSTEM_SVC_DIR" "$DBUS_SESSION_SVC_DIR" "$FORGE_BACKUP_DIR"
if [[ -f "${DESTDIR}/usr/share/dbus-1/system-services/org.freedesktop.systemd1.service" \
  && ! -f "$FORGE_BACKUP_DIR/org.freedesktop.systemd1.service.stock" ]]; then
  cp -a "${DESTDIR}/usr/share/dbus-1/system-services/org.freedesktop.systemd1.service" \
    "$FORGE_BACKUP_DIR/org.freedesktop.systemd1.service.stock"
fi
if [[ -e "$DBUS_SESSION_SVC_DIR/org.freedesktop.systemd1.service" \
  && ! -f "$FORGE_BACKUP_DIR/session-org.freedesktop.systemd1.service.stock" ]]; then
  cp -aL "$DBUS_SESSION_SVC_DIR/org.freedesktop.systemd1.service" \
    "$FORGE_BACKUP_DIR/session-org.freedesktop.systemd1.service.stock"
fi
SYSTEMD1_DBUS_SVC="$ROOT/packaging/dbus/org.freedesktop.systemd1.service"
if [[ "$INSTALL_NATIVE" == "1" || "$INSTALL_NATIVE" == "true" ]]; then
  SYSTEMD1_DBUS_SVC="$ROOT/packaging/dbus/org.freedesktop.systemd1-native.service"
fi
install -m 0644 "$SYSTEMD1_DBUS_SVC" \
  "$DBUS_SVC_DIR/org.freedesktop.systemd1.service"
# System bus scans /usr/share before /etc — override must replace the stock file.
install -m 0644 "$SYSTEMD1_DBUS_SVC" \
  "$DBUS_SYSTEM_SVC_DIR/org.freedesktop.systemd1.service"
# Session bus scans /usr/share before /etc — override must replace the stock file.
install -m 0644 "$ROOT/packaging/dbus/session-org.freedesktop.systemd1.service" \
  "$DBUS_SESSION_SVC_DIR/org.freedesktop.systemd1.service"
install -m 0755 "$ROOT/packaging/pam/forge-session-open" "${DESTDIR}/usr/libexec/forge-session-open"
install -m 0755 "$ROOT/packaging/pam/forge-session-close" "${DESTDIR}/usr/libexec/forge-session-close"
install -m 0755 "$ROOT/packaging/pam/pam-forge-login-session.sh" "$LIBEXECDIR/pam-forge-login-session.sh"
install -m 0755 "$ROOT/packaging/pam/pam-logind-create-session.py" "$LIBEXECDIR/pam-logind-create-session.py"
install -d "${DESTDIR}/usr/share/forge"
install -m 0644 "$ROOT/packaging/pam/login-forge-snippet" "${DESTDIR}/usr/share/forge/login-forge-snippet"
install -m 0755 "$ROOT/scripts/forge-pam-enable.sh" "${DESTDIR}/usr/sbin/forge-pam-enable"
install -m 0644 "$ROOT/packaging/pam/login-forge-snippet" "${DESTDIR}/usr/sbin/login-forge-snippet"
install -d "${DESTDIR}/etc/pam.d"
install -m 0644 "$ROOT/packaging/pam/forge" "${DESTDIR}/etc/pam.d/forge"

install -m 0644 "$ROOT/forge-core/examples/default.target" "$SYSCONFDIR/default.target"
install -m 0644 "$ROOT/forge-core/examples/network.toml" "$SYSCONFDIR/network.toml"
install -m 0644 "$ROOT/forge-core/examples/desktop.toml" "$SYSCONFDIR/desktop.toml"
install -m 0644 "$ROOT/forge-core/examples/boot.rhai" "$SYSCONFDIR/boot.rhai"

case "$INSTALL_UNITS" in
  replace)
    # Remove stale units so names don't collide across native vs forge.toml layouts.
    find "$UNITDIR" -mindepth 1 -delete 2>/dev/null || rm -rf "${UNITDIR:?}/"*
    cp -a "$UNIT_SOURCE/." "$UNITDIR/"
    if [[ "$INSTALL_NATIVE" == "1" || "$INSTALL_NATIVE" == "true" ]]; then
      chmod +x "$ROOT/scripts/forge-rc-update-seed.sh" 2>/dev/null || true
      "$ROOT/scripts/forge-rc-update-seed.sh" "$UNIT_SOURCE" 2>/dev/null || true
    fi
    ;;
  if-empty)
    if [[ -z "$(ls -A "$UNITDIR" 2>/dev/null || true)" ]]; then
      cp -a "$UNIT_SOURCE/." "$UNITDIR/"
      if [[ "$INSTALL_NATIVE" == "1" || "$INSTALL_NATIVE" == "true" ]]; then
        "$ROOT/scripts/forge-rc-update-seed.sh" "$UNIT_SOURCE" 2>/dev/null || true
      fi
    else
      echo "Keeping existing units in $UNITDIR (set FORGE_INSTALL_UNITS=replace to overwrite)"
    fi
    ;;
  *)
    echo "Unknown FORGE_INSTALL_UNITS=$INSTALL_UNITS (use replace or if-empty)" >&2
    exit 1
    ;;
esac

cp -a "$ROOT/forge-core/examples/systemd/." "${DESTDIR}/etc/forge/systemd/" 2>/dev/null || true

cat <<EOF
Installed (CIQ RLC Pro profile):
  $SBINDIR/forge-core              (PID 1 init)
  $BINDIR/forgectl                 (control CLI)
  $BINDIR/forge-logind             (session manager)
  $BINDIR/forge-journalctl         (journal query)
  $LIBEXECDIR/setup-net.sh         (Wi-Fi / NetworkManager bring-up)
  $LIBEXECDIR/start-agetty.sh      (agetty path resolver)
  $LIBEXECDIR/start-gdm.sh         (GDM path resolver)
  $LIBEXECDIR/start-polkit.sh      (polkitd launcher)
  $LIBEXECDIR/start-accounts-daemon.sh
  $LIBEXECDIR/systemd1-stub.py           (system org.freedesktop.systemd1 for logind)
  $LIBEXECDIR/systemd1-session-stub.py  (session org.freedesktop.systemd1 for GDM)
  ${DESTDIR}/etc/dbus-1/system-services/org.freedesktop.systemd1.service
  ${DESTDIR}/usr/share/dbus-1/services/org.freedesktop.systemd1.service
  $UNITDIR/                        ($UNIT_KIND units — $INSTALL_UNITS)
  ${DESTDIR}/etc/forge/systemd/    (systemd-compatible units)
  $SYSCONFDIR/default.target       (graphical)
  $SYSCONFDIR/network.toml         (defer Wi-Fi to network-setup)

GRUB / recovery:
  ${DESTDIR}/usr/sbin/forge-boot-enable   (grubby --update-kernel=ALL --args init=forge-core)
  ${DESTDIR}/usr/sbin/forge-boot-disable  (remove init= from all kernels)

After install, run: sudo forge-boot-enable
Then reboot. Recovery stays on Forge by default (set FORGE_RECOVERY_HANDOFF=1 for systemd fallback).
EOF

if [[ -z "$DESTDIR" && "${EUID:-$(id -u)}" -eq 0 ]]; then
  echo "Configuring login PAM for forge PID 1..."
  /usr/sbin/forge-pam-enable || echo "WARN: forge-pam-enable failed — run manually after install"
  if command -v semanage >/dev/null 2>&1 && [[ -d /sys/fs/selinux ]]; then
    echo "Configuring SELinux policy label for forge-core..."
    semanage fcontext -a -t init_exec_t "/usr/sbin/forge-core" 2>/dev/null || true
    restorecon -v /usr/sbin/forge-core || true
  elif command -v chcon >/dev/null 2>&1 && [[ -d /sys/fs/selinux ]]; then
    echo "Warning: semanage not found, applying temporary chcon label..."
    chcon -t init_exec_t "/usr/sbin/forge-core" || true
  fi
fi

if [[ -z "$DESTDIR" && "${FORGE_SKIP_GRUB:-}" != "1" && "${EUID:-$(id -u)}" -eq 0 ]]; then
  echo "Configuring GRUB (forge-boot-enable)..."
  /usr/sbin/forge-boot-enable || echo "WARN: forge-boot-enable failed — run manually after install"
elif [[ -z "$DESTDIR" ]]; then
  echo "Run as root to configure GRUB: sudo forge-boot-enable"
fi