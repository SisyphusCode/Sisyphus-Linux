#!/usr/bin/env bash
# Launch COSMIC greeter with explicit logind session (Forge — no greetd/pam_systemd).
set -euo pipefail

LOG=/var/log/forge/cosmic-greeter.log
mkdir -p /var/log/forge /run/cosmic-greeter
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) start-cosmic-greeter ==="

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"
for _ in $(seq 1 200); do
    [[ -S /run/dbus/system_bus_socket ]] && break
    sleep 0.1
done
for _ in $(seq 1 100); do
    busctl --address="$BUS" status org.freedesktop.login1 >/dev/null 2>&1 && break
    sleep 0.1
done
for _ in $(seq 1 100); do
    busctl --address="$BUS" status org.freedesktop.systemd1 >/dev/null 2>&1 && break
    sleep 0.1
done

if [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
    /usr/libexec/forge/release-graphics.sh || true
fi

if getent passwd cosmic-greeter >/dev/null 2>&1; then
    cg_uid="$(id -u cosmic-greeter)"
    mkdir -p /run/cosmic-greeter "/run/user/${cg_uid}"
    chown cosmic-greeter:cosmic-greeter /run/cosmic-greeter "/run/user/${cg_uid}"
    chmod 0755 /run/cosmic-greeter
    chmod 0700 "/run/user/${cg_uid}"
fi

if getent passwd cosmic-greeter >/dev/null 2>&1; then
    CG_UID="$(id -u cosmic-greeter)"
    CG_RUNTIME="/run/user/${CG_UID}"
    CG_BUS="unix:path=${CG_RUNTIME}/bus"
    mkdir -p "$CG_RUNTIME" /run/cosmic-greeter
    chown cosmic-greeter:cosmic-greeter "$CG_RUNTIME" /run/cosmic-greeter
    chmod 0700 "$CG_RUNTIME"
    chmod 0755 /run/cosmic-greeter

    busctl --address="$BUS" call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
        org.freedesktop.systemd1.Manager StartUnit ss "user@${CG_UID}.service" "replace" \
        >/dev/null 2>&1 || true
    for _ in $(seq 1 150); do
        [[ -S "${CG_RUNTIME}/bus" ]] && break
        sleep 0.1
    done

    if command -v cosmic-settings-daemon >/dev/null 2>&1; then
        if ! busctl --address="$BUS" status com.system76.CosmicSettingsDaemon >/dev/null 2>&1; then
            env XDG_RUNTIME_DIR="$CG_RUNTIME" /usr/bin/cosmic-settings-daemon >>"$LOG" 2>&1 &
            sleep 0.5
        fi
    fi

    if command -v cosmic-greeter-daemon >/dev/null 2>&1; then
        if ! busctl --address="$BUS" status com.system76.CosmicGreeter >/dev/null 2>&1; then
            /usr/bin/cosmic-greeter-daemon >>"$LOG" 2>&1 &
            for _ in $(seq 1 100); do
                busctl --address="$BUS" status com.system76.CosmicGreeter >/dev/null 2>&1 && break
                sleep 0.1
            done
        fi
    fi
fi

exec python3 /usr/libexec/forge/forge-cosmic-greeter-session.py