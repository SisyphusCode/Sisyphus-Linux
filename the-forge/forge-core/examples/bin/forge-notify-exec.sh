#!/bin/sh
# Minimal sd_notify helper for Type=notify services
if [ -n "$NOTIFY_SOCKET" ]; then
  printf 'READY=1' | nc -U -w1 -u "$NOTIFY_SOCKET" 2>/dev/null || true
fi
exec "$@"
