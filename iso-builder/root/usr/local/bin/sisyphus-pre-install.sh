#!/bin/bash
mkdir -p /run/rootfsbase
if ! mountpoint -q /run/rootfsbase; then
    # Find the squashfs device automatically and mount it
    SQ_DEV=$(lsblk -lno NAME,FSTYPE | grep squashfs | awk '{print "/dev/"$1}' | head -n 1)
    if [ -n "$SQ_DEV" ]; then
        mount "$SQ_DEV" /run/rootfsbase
    fi
fi
