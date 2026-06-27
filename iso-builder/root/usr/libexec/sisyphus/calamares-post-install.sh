#!/usr/bin/env bash
# Calamares chroot hook — configure Forge init and COSMIC greeter on installed system.
set -euo pipefail

ROOT="${CALAMARES_ROOT_MOUNT_POINT:-/}"

restore_forge_units() {
    cat > "${ROOT}/etc/forge/units/02-dbus-system.forge.toml" <<'EOF'
[service]
name = "dbus"
exec-start-pre = "/usr/libexec/forge/init-machine-id.sh"
exec = "/usr/libexec/forge/start-dbus.sh"
args = []
type = "dbus"
bus-name = "org.freedesktop.DBus"
after = ["forge-early"]
restart = "on-failure"

[service.environment]
DBUS_SYSTEM_BUS_ADDRESS = "unix:path=/run/dbus/system_bus_socket"
EOF

    cat > "${ROOT}/etc/forge/units/55-systemd1-stub.forge.toml" <<'EOF'
[service]
name = "systemd1-stub"
exec = "/usr/libexec/forge/systemd1-stub-wrapper.sh"
args = []
type = "dbus"
bus-name = "org.freedesktop.systemd1"
after = ["dbus"]
restart = "on-failure"
EOF

    cat > "${ROOT}/etc/forge/units/60-display-manager.forge.toml" <<'EOF'
[service]
name = "display-manager"
description = "COSMIC greeter display manager"
exec-start-pre = "/usr/libexec/forge/desktop-ready.sh"
exec = "/usr/libexec/forge/start-cosmic-greeter.sh"
args = []
type = "simple"
after = [
    "multi-user",
    "dbus",
    "udev",
    "udev-settle",
    "logind",
    "polkit",
    "user-sessions",
    "accounts-daemon",
    "network-setup",
]
restart = "on-failure"
EOF

    rm -f "${ROOT}/etc/forge/units/01-dbus.socket.forge.toml"

    if [[ -f "${ROOT}/etc/forge/units/05-logind.forge.toml" ]] \
        && ! grep -q 'systemd1-stub' "${ROOT}/etc/forge/units/05-logind.forge.toml"; then
        sed -i 's/after = \["forge-early", "dbus", "udev"\]/requires = ["systemd1-stub"]\nafter = ["forge-early", "dbus", "systemd1-stub", "udev"]/' \
            "${ROOT}/etc/forge/units/05-logind.forge.toml"
    fi
}

echo graphical > "${ROOT}/etc/forge/default.target"
restore_forge_units

if [[ -x "${ROOT}/usr/bin/forgectl" ]]; then
    for svc in dbus udev logind polkit accounts-daemon network-setup \
               network-manager user-sessions display-manager; do
        chroot "${ROOT}" forgectl enable "${svc}" 2>/dev/null || true
    done
fi

if ! getent passwd -R "${ROOT}" cosmic-greeter >/dev/null 2>&1; then
    chroot "${ROOT}" useradd -r -d /var/lib/cosmic-greeter -s /sbin/nologin \
        -c "Cosmic Greeter Account" cosmic-greeter 2>/dev/null || true
fi

if getent group -R "${ROOT}" video >/dev/null 2>&1; then
    chroot "${ROOT}" usermod -aG video cosmic-greeter 2>/dev/null || true
fi

mkdir -p "${ROOT}/var/lib/cosmic-greeter" "${ROOT}/run/cosmic-greeter"
cg_uid="$(chroot "${ROOT}" id -u cosmic-greeter 2>/dev/null || echo "")"
if [[ -n "${cg_uid}" ]]; then
    chown "${cg_uid}:${cg_uid}" "${ROOT}/var/lib/cosmic-greeter" 2>/dev/null || true
    for id in com.system76.CosmicComp com.system76.CosmicSettings.Shortcuts com.system76.CosmicSettings.WindowRules com.system76.CosmicTk; do
        mkdir -p "${ROOT}/var/lib/cosmic-greeter/.config/cosmic/${id}/v1"
    done
    chown -R "${cg_uid}:${cg_uid}" "${ROOT}/var/lib/cosmic-greeter"
    chmod 0755 "${ROOT}/var/lib/cosmic-greeter"
fi

if [[ -f "${ROOT}/etc/default/grub" ]] \
    && ! grep -q 'forge-core' "${ROOT}/etc/default/grub"; then
    sed -i 's/^GRUB_CMDLINE_LINUX="/GRUB_CMDLINE_LINUX="init=\/usr\/sbin\/forge-core selinux=0 /' \
        "${ROOT}/etc/default/grub"
fi

rm -f "${ROOT}/etc/sisyphus/installer-enabled"
exit 0