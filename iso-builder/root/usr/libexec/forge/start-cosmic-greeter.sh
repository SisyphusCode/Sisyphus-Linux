#!/usr/bin/env bash
# Launch COSMIC greeter with explicit logind session (Forge — no greetd/pam_systemd).
set -euo pipefail

LOG=/var/log/forge/cosmic-greeter.log
mkdir -p /var/log/forge /run/cosmic-greeter
exec >>"$LOG" 2>&1
echo "=== $(date -Is 2>/dev/null || date) start-cosmic-greeter ==="
echo "DEBUG: Checking if cosmic-greeter user exists..."
if getent passwd cosmic-greeter >/dev/null 2>&1; then
    echo "DEBUG: cosmic-greeter user exists"
else
    echo "DEBUG: ERROR - cosmic-greeter user does NOT exist"
fi
echo "DEBUG: Checking for cosmic-comp..."
if command -v cosmic-comp >/dev/null 2>&1; then
    echo "DEBUG: cosmic-comp found at $(command -v cosmic-comp)"
else
    echo "DEBUG: ERROR - cosmic-comp NOT found"
fi
echo "DEBUG: Checking for cosmic-greeter..."
if command -v cosmic-greeter >/dev/null 2>&1; then
    echo "DEBUG: cosmic-greeter found at $(command -v cosmic-greeter)"
else
    echo "DEBUG: ERROR - cosmic-greeter NOT found"
fi
echo "DEBUG: Checking for pop-sound-theme..."
if rpm -q pop-sound-theme >/dev/null 2>&1; then
    echo "DEBUG: pop-sound-theme is installed"
else
    echo "DEBUG: ERROR - pop-sound-theme is NOT installed"
fi

BUS="${DBUS_SYSTEM_BUS_ADDRESS:-unix:path=/run/dbus/system_bus_socket}"

dbus_name_owned() {
    local name="$1"
    if command -v busctl >/dev/null 2>&1; then
        busctl --address="$BUS" call org.freedesktop.DBus /org/freedesktop/DBus \
            org.freedesktop.DBus NameHasOwner s "$name" 2>/dev/null | grep -q 'true'
        return $?
    fi
    dbus-send --address="$BUS" --dest=org.freedesktop.DBus --print-reply \
        /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner "string:$name" 2>/dev/null \
        | grep -q 'boolean true'
}

for _ in $(seq 1 50); do
    [[ -S /run/dbus/system_bus_socket ]] && break
    sleep 0.1
done
for _ in $(seq 1 50); do
    dbus_name_owned org.freedesktop.login1 && break
    sleep 0.1
done
for _ in $(seq 1 50); do
    dbus_name_owned org.freedesktop.systemd1 && break
    sleep 0.1
done

# Aggressive cleanup of stale sockets from previous crashed greeter attempts (Xwayland + wayland)
rm -f /tmp/.X[0-9]*-lock /tmp/.X11-unix/X* 2>/dev/null || true
rm -rf /tmp/wayland-* 2>/dev/null || true

if [[ -x /usr/libexec/forge/release-graphics.sh ]]; then
    /usr/libexec/forge/release-graphics.sh || true
fi

prepare_cosmic_greeter_dirs() {
    local cg_uid="$1"
    local shortcuts_cfg="/var/lib/cosmic-greeter/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1"
    local shortcuts_share="/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1"

    mkdir -p /run/cosmic-greeter/cosmic "/run/user/${cg_uid}" \
        /var/lib/cosmic-greeter/.config \
        /var/lib/cosmic-greeter/.local/state \
        /var/lib/cosmic-greeter/.local/share \
        "${shortcuts_cfg}"
    # Pre-create common cosmic config trees so cosmic-config::Config::new never fails for greeter
    for id in com.system76.CosmicComp com.system76.CosmicSettings.Shortcuts com.system76.CosmicSettings.WindowRules com.system76.CosmicTk com.system76.CosmicGreeter com.system76.CosmicSettingsDaemon; do
        mkdir -p "/var/lib/cosmic-greeter/.config/cosmic/${id}/v1"
    done
    if [[ -d "${shortcuts_share}" ]]; then
        cp -af "${shortcuts_share}/." "${shortcuts_cfg}/" 2>/dev/null || true
    fi
    chown -R cosmic-greeter:cosmic-greeter /var/lib/cosmic-greeter /run/cosmic-greeter
    chown cosmic-greeter:cosmic-greeter "/run/user/${cg_uid}"
    chmod 0755 /var/lib/cosmic-greeter
    chmod 0755 /run/cosmic-greeter
    chmod 0700 "/run/user/${cg_uid}"
    touch /var/log/forge/cosmic-greeter-session.log
    chown cosmic-greeter:cosmic-greeter /var/log/forge/cosmic-greeter-session.log
    chmod 0644 /var/log/forge/cosmic-greeter-session.log

    # Extra guarantee: make dri devices accessible to the greeter (in case udev/early ACLs missed in this boot)
    for dri in /dev/dri/card* /dev/dri/renderD*; do
        [[ -e "$dri" ]] || continue
        chgrp video "$dri" 2>/dev/null || true
        chmod 0666 "$dri" 2>/dev/null || true
        setfacl -m "u:cosmic-greeter:rw" "$dri" 2>/dev/null || true
    done

    # Clean any stale wayland sockets in the greeter runtime (as root)
    rm -f "/run/user/${cg_uid}/wayland-"* 2>/dev/null || true
}

if getent passwd cosmic-greeter >/dev/null 2>&1; then
    cg_uid="$(id -u cosmic-greeter)"
    prepare_cosmic_greeter_dirs "${cg_uid}"
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
    for _ in $(seq 1 50); do
        busctl --address="$CG_BUS" call org.freedesktop.DBus /org/freedesktop/DBus \
            org.freedesktop.DBus NameHasOwner s org.freedesktop.systemd1 2>/dev/null \
            | grep -q 'true' && break
        sleep 0.1
    done

    greeter_env=(
        "HOME=/var/lib/cosmic-greeter"
        "XDG_CONFIG_HOME=/var/lib/cosmic-greeter/.config"
        "XDG_STATE_HOME=/var/lib/cosmic-greeter/.local/state"
        "XDG_DATA_HOME=/var/lib/cosmic-greeter/.local/share"
        "XDG_RUNTIME_DIR=${CG_RUNTIME}"
        "DBUS_SESSION_BUS_ADDRESS=${CG_BUS}"
    )

    if command -v cosmic-settings-daemon >/dev/null 2>&1; then
        for _ in $(seq 1 100); do
            busctl --address="$CG_BUS" call org.freedesktop.DBus /org/freedesktop/DBus \
                org.freedesktop.DBus NameHasOwner s org.freedesktop.systemd1 2>/dev/null \
                | grep -q 'true' && break
            sleep 0.1
        done
        if ! busctl --address="$CG_BUS" status com.system76.CosmicSettingsDaemon >/dev/null 2>&1; then
            for attempt in $(seq 1 3); do
                runuser -u cosmic-greeter -- env "${greeter_env[@]}" \
                    /usr/bin/cosmic-settings-daemon >>"$LOG" 2>&1 &
                for _ in $(seq 1 30); do
                    busctl --address="$CG_BUS" status com.system76.CosmicSettingsDaemon >/dev/null 2>&1 && break 2
                    sleep 0.2
                done
                echo "start-cosmic-greeter: cosmic-settings-daemon not on bus (attempt ${attempt})" >>"$LOG"
            done
            chown -R cosmic-greeter:cosmic-greeter /run/cosmic-greeter/cosmic 2>/dev/null || true
        fi
    fi

    if command -v cosmic-greeter-daemon >/dev/null 2>&1; then
        if ! busctl --address="$BUS" status com.system76.CosmicGreeter >/dev/null 2>&1; then
            # D-Bus policy allows only root to own com.system76.CosmicGreeter.
            echo "start-cosmic-greeter: launching cosmic-greeter-daemon (as root)" >>"$LOG"
            /usr/bin/cosmic-greeter-daemon >>"$LOG" 2>&1 &
            DAEMON_PID=$!
            echo "start-cosmic-greeter: cosmic-greeter-daemon pid=$DAEMON_PID" >>"$LOG"
            for _ in $(seq 1 150); do
                if busctl --address="$BUS" status com.system76.CosmicGreeter >/dev/null 2>&1; then
                    echo "start-cosmic-greeter: CosmicGreeter name acquired" >>"$LOG"
                    break
                fi
                sleep 0.1
            done
            if ! busctl --address="$BUS" status com.system76.CosmicGreeter >/dev/null 2>&1; then
                echo "start-cosmic-greeter: WARNING CosmicGreeter still not on bus after wait" >>"$LOG"
            fi
        fi
    fi
fi

# Forge logind session + cosmic-comp (greetd IPC is not used on Forge PID 1 live media).
exec python3 /usr/libexec/forge/forge-cosmic-greeter-session.py