#!/usr/bin/sh
if getargbool 0 forge.init; then
    info "The Forge: using forge-core as init"
    ln -sf init "${initdir}/init"
fi
