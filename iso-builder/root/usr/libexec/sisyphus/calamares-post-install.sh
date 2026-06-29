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
description = "greetd COSMIC display manager"
exec-start-pre = "/usr/libexec/forge/desktop-ready.sh"
exec = "/usr/bin/greetd"
args = ["-c", "/etc/greetd/cosmic-greeter.toml"]
type = "simple"
requires = ["seatd", "localed-stub", "cosmic-greeter-daemon"]
after = [
    "multi-user",
    "dbus",
    "udev",
    "udev-settle",
    "logind",
    "localed-stub",
    "polkit",
    "seatd",
    "cosmic-greeter-daemon",
    "user-sessions",
    "accounts-daemon",
    "network-setup",
]
restart = "on-failure"
EOF

    cat > "${ROOT}/etc/forge/units/54-localed-stub.forge.toml" <<'EOF'
[service]
name = "localed-stub"
description = "Minimal org.freedesktop.locale1 provider"
exec = "/usr/libexec/forge/localed-stub.py"
args = []
type = "dbus"
bus-name = "org.freedesktop.locale1"
after = ["dbus"]
restart = "on-failure"
EOF

    cat > "${ROOT}/etc/forge/units/56-seatd.forge.toml" <<'EOF'
[service]
name = "seatd"
description = "Seat management daemon"
exec = "/usr/bin/seatd"
args = ["-g", "seat"]
type = "simple"
after = ["forge-early", "udev", "udev-settle"]
restart = "on-failure"
EOF

    cat > "${ROOT}/etc/forge/units/57-cosmic-greeter-daemon.forge.toml" <<'EOF'
[service]
name = "cosmic-greeter-daemon"
description = "COSMIC greeter system daemon"
exec = "/usr/bin/cosmic-greeter-daemon"
args = []
type = "dbus"
bus-name = "com.system76.CosmicGreeter"
after = ["dbus"]
restart = "on-failure"
EOF

    cat > "${ROOT}/etc/forge/units/58-wpa_supplicant.forge.toml" <<'EOF'
[service]
name = "wpa_supplicant"
description = "WPA Supplicant daemon"
exec = "/usr/sbin/wpa_supplicant"
args = ["-c", "/etc/wpa_supplicant/wpa_supplicant.conf", "-u"]
type = "dbus"
bus-name = "fi.w1.wpa_supplicant1"
after = ["dbus"]
restart = "on-failure"
EOF

    cat > "${ROOT}/etc/forge/units/00-multi-user.target.forge.toml" <<'EOF'
[target]
name = "multi-user"
requires = ["forge-early", "dbus", "polkit", "udev", "logind"]
wants = [
    "plymouth-kill",
    "udev-trigger",
    "udev-settle",
    "NetworkManager",
    "network-setup",
    "wpa_supplicant",
    "user-sessions",
    "getty-console",
]
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
    for svc in dbus udev logind localed-stub polkit accounts-daemon network-setup \
               network-manager wpa_supplicant user-sessions seatd cosmic-greeter-daemon display-manager; do
        chroot "${ROOT}" forgectl enable "${svc}" 2>/dev/null || true
    done
fi

if ! getent group -R "${ROOT}" seat >/dev/null 2>&1; then
    chroot "${ROOT}" groupadd -r seat 2>/dev/null || true
fi

if ! getent passwd -R "${ROOT}" cosmic-greeter >/dev/null 2>&1; then
    chroot "${ROOT}" useradd -r -d /var/lib/cosmic-greeter -s /sbin/nologin \
        -c "Cosmic Greeter Account" cosmic-greeter 2>/dev/null || true
fi

if getent group -R "${ROOT}" video >/dev/null 2>&1; then
    chroot "${ROOT}" usermod -aG video cosmic-greeter 2>/dev/null || true
fi
for grp in render input seat; do
    if getent group -R "${ROOT}" "${grp}" >/dev/null 2>&1; then
        chroot "${ROOT}" usermod -aG "${grp}" cosmic-greeter 2>/dev/null || true
    fi
done

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