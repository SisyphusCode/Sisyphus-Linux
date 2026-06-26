#!/usr/bin/bash
# dracut module for The Forge init (install to /usr/lib/dracut/modules.d/90forge/)

check() {
    require_binaries forge-core || return 1
    return 0
}

depends() {
    echo base
    return 0
}

install() {
    local moddir="$1"

    if command -v forge-core >/dev/null 2>&1; then
        inst_simple "$(command -v forge-core)" /sbin/init
        inst_simple "$(command -v forgectl)" /usr/bin/forgectl
        inst_simple "$(command -v forge-logind)" /usr/bin/forge-logind
    else
        derror "forge-core not installed"
        return 1
    fi

    inst_dir /etc/forge
    inst_dir /etc/forge/units
    inst_dir /etc/forge/systemd
    inst_simple /etc/forge/default.target /etc/forge/default.target 2>/dev/null || true
    inst_simple /etc/forge/network.toml /etc/forge/network.toml 2>/dev/null || true

    if [[ -d /etc/forge/units ]]; then
        inst_multiple /etc/forge/units/*
    fi
    if [[ -d /etc/forge/systemd ]]; then
        inst_multiple /etc/forge/systemd/*
    fi

    if command -v busybox >/dev/null 2>&1; then
        inst_simple "$(command -v busybox)" /bin/busybox
    fi

    inst_hook cmdline 30 "$moddir/forge-cmdline.sh"
    dracut_need_initrd 2>/dev/null || true
}
