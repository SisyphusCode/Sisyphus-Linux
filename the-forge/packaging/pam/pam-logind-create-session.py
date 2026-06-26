#!/usr/bin/env python3
"""Create a logind session for forge PID 1 tty login (pam_systemd equivalent)."""
import os
import sys


def _log(msg: str) -> None:
    try:
        with open("/var/log/forge/pam-login.log", "a", encoding="utf-8") as fh:
            fh.write(msg + "\n")
    except OSError:
        pass


def _vtnr_and_tty():
    tty = os.environ.get("PAM_TTY", "")
    if not tty:
        return 0, ""
    if not tty.startswith("/"):
        tty = f"/dev/{tty}"
    base = tty.rsplit("/", 1)[-1]
    if base.startswith("tty") and base[3:].isdigit():
        return int(base[3:]), tty
    return 0, tty


def main() -> int:
    try:
        import dbus
    except ImportError:
        _log("python3-dbus missing")
        return 0

    user = os.environ.get("PAM_USER", "")
    uid = int(os.environ.get("PAM_UID", "0") or "0")
    service = os.environ.get("PAM_SERVICE", "login")
    vtnr, tty = _vtnr_and_tty()
    leader = os.getpid()
    bus_addr = os.environ.get(
        "DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/run/dbus/system_bus_socket"
    )

    try:
        bus = dbus.SystemBus(private=bus_addr)
        bus.set_exit_on_disconnect(False)
        mgr = bus.get_object(
            "org.freedesktop.login1", "/org/freedesktop/login1"
        )
        iface = dbus.Interface(
            mgr, "org.freedesktop.login1.Manager"
        )
        for sid in iface.ListSessions():
            if len(sid) >= 2 and str(sid[1]) == user:
                _log(f"session already exists for {user}: {sid[0]}")
                return 0
        # systemd 252 CreateSession(uusssssussbssa(sv)) -> (soshusub)
        result = iface.CreateSession(
            dbus.UInt32(uid),
            dbus.UInt32(leader),
            service,
            "tty",
            "user",
            "",
            "",
            dbus.UInt32(vtnr),
            tty,
            "",
            dbus.Boolean(False),
            "",
            "",
            dbus.Array([], signature="(sv)"),
        )
        session_id = result[0] if result else "?"
        runtime = result[2] if len(result) > 2 else ""
        _log(
            f"CreateSession ok user={user} vtnr={vtnr} tty={tty} "
            f"id={session_id} runtime={runtime}"
        )
    except Exception as exc:  # noqa: BLE001
        msg = str(exc)
        if "Already running" in msg or "already exists" in msg.lower():
            _log(f"CreateSession skipped (existing session) user={user}")
            return 0
        _log(f"CreateSession failed user={user} vtnr={vtnr} tty={tty}: {exc!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())