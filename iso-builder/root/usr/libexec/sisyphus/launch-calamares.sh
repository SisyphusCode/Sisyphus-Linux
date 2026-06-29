#!/usr/bin/env bash
export QT_QPA_PLATFORM=wayland
export QT_WAYLAND_DISABLE_WINDOWDECORATION=1
exec sudo -E calamares -d
