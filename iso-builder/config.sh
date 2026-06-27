#!/bin/bash
# Sisyphus Linux — Kiwi chroot customization (runs as root inside the image).
set -euxo pipefail

echo "==> Sisyphus Linux post-install configuration"

# Branding
echo "sisyphus-linux" > /etc/hostname
cat > /etc/os-release <<'EOF'
NAME="Sisyphus Linux"
PRETTY_NAME="Sisyphus Linux 0.1.0"
ID=sisyphus
ID_LIKE=fedora
VERSION_ID=0.1.0
VERSION="0.1.0 (Forge)"
HOME_URL="https://github.com/SisyphusCode/the-forge"
SUPPORT_URL="https://github.com/SisyphusCode/the-forge/issues"
BUG_REPORT_URL="https://github.com/SisyphusCode/the-forge/issues"
EOF

if [[ ! -e /usr/lib/os-release ]] || [[ "$(readlink -f /usr/lib/os-release)" != "$(readlink -f /etc/os-release)" ]]; then
    ln -sf /etc/os-release /usr/lib/os-release
fi

# Forge early-boot expects gdm user/group for /run/gdm layout (even with COSMIC greeter)
if ! getent group gdm >/dev/null 2>&1; then
    groupadd -r gdm
fi
if ! getent passwd gdm >/dev/null 2>&1; then
    useradd -r -g gdm -d /var/lib/gdm -s /sbin/nologin -c "GDM Greeter" gdm
    mkdir -p /var/lib/gdm
    chown gdm:gdm /var/lib/gdm
fi

# systemd-sysusers does not run under Forge — create COSMIC greeter account manually
if ! getent passwd cosmic-greeter >/dev/null 2>&1; then
    useradd -r -d /var/lib/cosmic-greeter -s /sbin/nologin -c "Cosmic Greeter Account" cosmic-greeter
fi
for grp in video render input seat; do
    if getent group "$grp" >/dev/null 2>&1; then
        usermod -aG "$grp" cosmic-greeter 2>/dev/null || true
    fi
done
mkdir -p /var/lib/cosmic-greeter /run/cosmic-greeter
chown cosmic-greeter:cosmic-greeter /var/lib/cosmic-greeter /run/cosmic-greeter
chmod 0750 /var/lib/cosmic-greeter
chmod 0755 /run/cosmic-greeter
if command -v systemd-tmpfiles >/dev/null 2>&1; then
    systemd-tmpfiles --create /usr/lib/tmpfiles.d/cosmic-greeter.conf 2>/dev/null || true
fi

# Ensure forge helper scripts are executable
chmod 0755 /usr/libexec/forge/init-machine-id.sh 2>/dev/null || true
chmod 0755 /usr/bin/cosmic-greeter-start 2>/dev/null || true
chmod 0755 /usr/libexec/forge/start-cosmic-greeter.sh 2>/dev/null || true
chmod 0755 /usr/libexec/forge/pam-forge-login-session.sh 2>/dev/null || true
chmod 0755 /usr/libexec/forge/pam-logind-create-session.py 2>/dev/null || true
chmod 0755 /usr/libexec/forge/forge-cosmic-greeter-session.py 2>/dev/null || true
chmod 0755 /usr/libexec/forge/cosmic-greeter-setup.sh 2>/dev/null || true
chmod 0755 /usr/libexec/forge/desktop-ready.sh 2>/dev/null || true
chmod 0755 /usr/libexec/forge/*.sh 2>/dev/null || true
chmod 0755 /usr/libexec/sisyphus/*.sh 2>/dev/null || true

# Enable Calamares native installer on overlayroot live media (Kiwi OEM).
mkdir -p /etc/sisyphus
touch /etc/sisyphus/installer-enabled

# Forge owns display-manager — disable stock greetd/cosmic-greeter systemd units.
systemctl mask cosmic-greeter.service 2>/dev/null || \
    ln -sf /dev/null /etc/systemd/system/cosmic-greeter.service
rm -f /etc/systemd/system/display-manager.service

# Boot to graphical target with COSMIC greeter stack.
echo graphical > /etc/forge/default.target
if command -v forgectl >/dev/null 2>&1; then
    for svc in dbus udev logind polkit accounts-daemon network-setup \
               network-manager user-sessions display-manager sisyphus-installer; do
        forgectl enable "$svc" 2>/dev/null || true
    done
fi

# COPR GPG keys for on-image dnf upgrades
if command -v rpm >/dev/null 2>&1; then
    rpm --import https://download.copr.fedorainfracloud.org/results/sisyphuscode/the-forge/pubkey.gpg 2>/dev/null || true
    rpm --import https://download.copr.fedorainfracloud.org/results/sisyphuscode/tuned-rs/pubkey.gpg 2>/dev/null || true
fi

# Rebuild initramfs with the Forge dracut module
if command -v dracut >/dev/null 2>&1; then
    KERNEL=$(ls -1 /lib/modules 2>/dev/null | tail -1)
    if [[ -n "${KERNEL}" ]]; then
        dracut --force --kver "${KERNEL}" \
            --add "90forge" \
            --install "forge-core forgectl forge-logind" \
            2>&1 || dracut --force --regenerate-all 2>&1 || true
    else
        dracut --force --regenerate-all 2>&1 || true
    fi
fi

# GRUB kernel args: Forge as PID 1
if [[ -f /etc/default/grub ]]; then
    if ! grep -q 'forge-core' /etc/default/grub; then
        sed -i 's/^GRUB_CMDLINE_LINUX="/GRUB_CMDLINE_LINUX="init=\/usr\/sbin\/forge-core selinux=0 /' /etc/default/grub
    fi
    if command -v grub2-mkconfig >/dev/null 2>&1 && [[ -d /boot/grub2 ]]; then
        grub2-mkconfig -o /boot/grub2/grub.cfg 2>/dev/null || true
    fi
fi

# Optional local the-forge RPM override (COPR ships 2.0.0-7 by default).
if ls /root/the-forge-*.rpm >/dev/null 2>&1; then
    rpm -Uvh --replacepkgs /root/the-forge-*.rpm 2>/dev/null \
        || dnf -y upgrade /root/the-forge-*.rpm 2>/dev/null \
        || true
fi

# Restore Sisyphus forge units after the-forge RPM (stock units use GDM + dbus.socket).
cat > /etc/forge/units/02-dbus-system.forge.toml <<'EOF'
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

cat > /etc/forge/units/60-display-manager.forge.toml <<'EOF'
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

cat > /etc/forge/units/55-systemd1-stub.forge.toml <<'EOF'
[service]
name = "systemd1-stub"
exec = "/usr/libexec/forge/systemd1-stub-wrapper.sh"
args = []
type = "dbus"
bus-name = "org.freedesktop.systemd1"
after = ["dbus"]
restart = "on-failure"
EOF

if ! grep -q 'systemd1-stub' /etc/forge/units/05-logind.forge.toml 2>/dev/null; then
    sed -i 's/after = \["forge-early", "dbus", "udev"\]/requires = ["systemd1-stub"]\nafter = ["forge-early", "dbus", "systemd1-stub", "udev"]/' \
        /etc/forge/units/05-logind.forge.toml 2>/dev/null || true
fi

rm -f /etc/forge/units/01-dbus.socket.forge.toml

# pop-sound-theme bundled in overlay — cosmic-settings-daemon expects it on Rawhide.
if ls /root/pop-sound-theme-*.rpm >/dev/null 2>&1; then
    dnf -y install /root/pop-sound-theme-*.rpm 2>/dev/null || true
fi

# Enable dnf COPR repos for runtime package installs
dnf -y makecache 2>/dev/null || true

# D-Bus machine-id last — dracut/dnf must not leave /etc/machine-id empty
machine_id=""
if [[ -s /var/lib/dbus/machine-id ]]; then
    machine_id="$(tr -d '\n' < /var/lib/dbus/machine-id)"
fi
if [[ ${#machine_id} -ne 32 ]] && command -v dbus-uuidgen >/dev/null 2>&1; then
    machine_id="$(dbus-uuidgen | tr -d '\n')"
fi
if [[ ${#machine_id} -ne 32 ]]; then
    machine_id="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
fi
mkdir -p /var/lib/dbus
printf '%s\n' "$machine_id" > /var/lib/dbus/machine-id
printf '%s\n' "$machine_id" > /etc/machine-id
chmod 0644 /etc/machine-id /var/lib/dbus/machine-id

echo "==> Sisyphus Linux configuration complete"