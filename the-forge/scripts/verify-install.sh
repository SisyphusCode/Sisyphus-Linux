#!/usr/bin/env bash
# Confirm Forge PID 1 install is current before rebooting.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORGE_INIT='init=/usr/sbin/forge-core'

ok=0
fail=0

pass() {
  echo "OK   $1"
  ok=$((ok + 1))
}

fail_check() {
  echo "FAIL $1"
  fail=$((fail + 1))
}

grubby_read() {
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    grubby "$@" 2>/dev/null
  elif command -v sudo >/dev/null 2>&1; then
    sudo grubby "$@" 2>/dev/null
  else
    grubby "$@" 2>/dev/null
  fi
}

forge_init_configured() {
  if grubby_read --info=DEFAULT | grep -qF "$FORGE_INIT"; then
    return 0
  fi
  if grubby_read --info=ALL | grep -qF "$FORGE_INIT"; then
    return 0
  fi
  if [[ -f /etc/default/grub ]] && grep -qF "$FORGE_INIT" /etc/default/grub; then
    return 0
  fi
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    grep -rlF "$FORGE_INIT" /boot/loader/entries/ >/dev/null 2>&1 && return 0
  elif command -v sudo >/dev/null 2>&1; then
    sudo grep -rlF "$FORGE_INIT" /boot/loader/entries/ >/dev/null 2>&1 && return 0
  fi
  return 1
}

# Detect native OpenRC-style unit layout vs legacy .forge.toml
NATIVE_MODE=0
if [[ -f /etc/forge/units/02-dbus.service.toml ]] \
  || [[ -f /etc/forge/units/02-dbus-system.forge.toml && -z "$(ls /etc/forge/units/*.service.toml 2>/dev/null)" ]]; then
  if [[ -f /etc/forge/units/02-dbus.service.toml ]]; then
    NATIVE_MODE=1
  fi
fi
if grep -q 'FORGE_NATIVE_MODE=1' /proc/cmdline 2>/dev/null; then
  NATIVE_MODE=1
fi

echo "Install profile: $([[ $NATIVE_MODE -eq 1 ]] && echo 'native OpenRC-style' || echo 'legacy forge.toml')"
echo

if [[ -x /usr/sbin/forge-core ]]; then
  pass "forge-core installed"
else
  fail_check "forge-core installed"
fi

if [[ -x "$ROOT/target/release/forge-core" ]] && cmp -s /usr/sbin/forge-core "$ROOT/target/release/forge-core" 2>/dev/null; then
  pass "forge-core matches local release build"
else
  fail_check "forge-core matches local release build (run: cargo build --release && sudo FORGE_SKIP_BUILD=1 ./scripts/install.sh)"
fi

if [[ -x /usr/sbin/forge-boot-enable && -x /usr/sbin/forge-boot-disable ]]; then
  pass "recovery scripts installed (forge-boot-enable/disable)"
else
  fail_check "recovery scripts installed"
fi

if grep -q graphical /etc/forge/default.target; then
  pass "default target graphical"
else
  fail_check "default target graphical"
fi

if [[ $NATIVE_MODE -eq 1 ]]; then
  if grep -q 'name = "dbus"' /etc/forge/units/02-dbus.service.toml 2>/dev/null \
    && grep -q 'org.freedesktop.DBus' /etc/forge/units/02-dbus.service.toml 2>/dev/null; then
    pass "dbus native unit configured"
  else
    fail_check "dbus native unit configured (02-dbus.service.toml)"
  fi

  if grep -q 'start-polkit.sh' /etc/forge/units/52-polkit.service.toml 2>/dev/null; then
    pass "polkit native unit uses path wrapper"
  else
    fail_check "polkit native unit uses path wrapper"
  fi

  if grep -q 'name = "sysinit"' /etc/forge/units/00-sysinit.target.toml 2>/dev/null; then
    pass "sysinit native target present"
  else
    fail_check "sysinit native target present"
  fi

  if grep -q 'sysinit' /etc/forge/units/00-multi-user.target.toml 2>/dev/null; then
    pass "multi-user requires sysinit"
  else
    fail_check "multi-user requires sysinit"
  fi

  if grep -q '^Exec=/bin/false' /usr/share/dbus-1/system-services/org.freedesktop.systemd1.service 2>/dev/null; then
    pass "system dbus systemd1 uses Exec=/bin/false (Rust stub owns name)"
  else
    fail_check "system dbus systemd1 should use Exec=/bin/false in native mode"
  fi
else
  if grep -q 'name = "dbus"' /etc/forge/units/02-dbus-system.forge.toml 2>/dev/null \
    && grep -q 'org.freedesktop.DBus' /etc/forge/units/02-dbus-system.forge.toml 2>/dev/null; then
    pass "dbus unit configured (forge-core runcon launcher)"
  else
    fail_check "dbus unit configured"
  fi

  if grep -q 'start-polkit.sh' /etc/forge/units/52-polkit.forge.toml 2>/dev/null; then
    pass "polkit unit uses path wrapper"
  else
    fail_check "polkit unit uses path wrapper"
  fi

  if [[ -x /usr/libexec/forge/systemd1-stub.py ]]; then
    pass "systemd1-stub installed"
  else
    fail_check "systemd1-stub installed"
  fi

  if grep -q 'systemd1-stub' /etc/forge/units/00-multi-user.target.forge.toml 2>/dev/null; then
    pass "multi-user requires systemd1-stub"
  else
    fail_check "multi-user requires systemd1-stub"
  fi

  if grep -q 'forge/systemd1-stub' /usr/share/dbus-1/system-services/org.freedesktop.systemd1.service 2>/dev/null; then
    pass "dbus org.freedesktop.systemd1 override (/usr/share)"
  else
    fail_check "dbus org.freedesktop.systemd1 override (/usr/share)"
  fi
fi

if [[ -x /usr/libexec/forge/start-udevd.sh ]]; then
  pass "start-udevd.sh installed"
else
  fail_check "start-udevd.sh installed"
fi

if forge_init_configured; then
  if [[ $NATIVE_MODE -eq 1 ]]; then
    if grubby_read --info=DEFAULT 2>/dev/null | grep -qF 'FORGE_NATIVE_MODE=1' \
      || grep -qF 'FORGE_NATIVE_MODE=1' /boot/loader/entries/*.conf 2>/dev/null; then
      pass "grub init=forge-core FORGE_NATIVE_MODE=1"
    else
      fail_check "grub missing FORGE_NATIVE_MODE=1 (run: sudo forge-boot-enable)"
    fi
  else
    pass "grub init=forge-core"
  fi
else
  fail_check "grub init=forge-core (run: sudo forge-boot-enable)"
fi

# Optional smoke test when running from a dev tree with release binary
if [[ -x "$ROOT/target/release/forge-core" && -d "$ROOT/forge-core/examples/native-desktop" ]]; then
  if FORGE_NATIVE_MODE=1 FORGE_UNIT_DIR="$ROOT/forge-core/examples/native-desktop" \
    FORGE_TARGET=multi-user timeout 20s "$ROOT/target/release/forge-core" 2>&1 | grep -q 'Boot complete'; then
    pass "native-desktop sandbox boot smoke test"
  else
    fail_check "native-desktop sandbox boot smoke test"
  fi
fi

echo
echo "Passed: $ok  Failed: $fail"
if [[ $fail -gt 0 ]]; then
  echo "Fix failures before rebooting with init=/usr/sbin/forge-core"
  exit 1
fi
echo "Install looks ready for PID 1 boot."