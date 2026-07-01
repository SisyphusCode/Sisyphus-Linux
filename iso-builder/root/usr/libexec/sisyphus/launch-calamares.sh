#!/usr/bin/env bash
set -euo pipefail

if command -v pgrep >/dev/null 2>&1 && pgrep -x calamares >/dev/null 2>&1; then
    exit 0
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" || -z "${WAYLAND_DISPLAY:-}" ]]; then
    for socket in /run/user/*/wayland-*; do
        [[ -S "$socket" ]] || continue
        export XDG_RUNTIME_DIR="$(dirname "$socket")"
        export WAYLAND_DISPLAY="${socket##*/}"
        break
    done
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" || -z "${WAYLAND_DISPLAY:-}" ]]; then
    echo "launch-calamares: no Wayland display found" >&2
    exit 1
fi

export QT_QPA_PLATFORM=wayland
export QT_WAYLAND_DISABLE_WINDOWDECORATION=1

if [[ "${EUID}" -eq 0 ]]; then
    exec calamares -d
fi

exec sudo -n --preserve-env=XDG_RUNTIME_DIR,WAYLAND_DISPLAY,QT_QPA_PLATFORM,QT_WAYLAND_DISABLE_WINDOWDECORATION calamares -d
