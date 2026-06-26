#!/usr/bin/env bash
# Kiwi truncates /etc/machine-id after image build; restore from /var/lib/dbus.
set -euo pipefail

valid_id() {
    local id="${1:-}"
    [[ ${#id} -eq 32 ]]
}

read_id() {
    local path="$1"
    [[ -s "$path" ]] || return 1
    tr -d '\n' <"$path"
}

id=""
if id="$(read_id /var/lib/dbus/machine-id 2>/dev/null)" && valid_id "$id"; then
    :
elif id="$(read_id /etc/machine-id 2>/dev/null)" && valid_id "$id"; then
    :
elif command -v dbus-uuidgen >/dev/null 2>&1; then
    id="$(dbus-uuidgen | tr -d '\n')"
else
    id="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
fi

mkdir -p /var/lib/dbus /run/dbus
printf '%s\n' "$id" > /var/lib/dbus/machine-id
printf '%s\n' "$id" > /etc/machine-id
chmod 0644 /etc/machine-id /var/lib/dbus/machine-id
# dbus-daemon runs as user dbus and must create the socket here (no socket activation)
if getent group dbus >/dev/null 2>&1; then
    chown root:dbus /run/dbus
    chmod 0775 /run/dbus
else
    chown root:root /run/dbus
    chmod 0755 /run/dbus
fi
rm -f /run/dbus/system_bus_socket /run/dbus/pid