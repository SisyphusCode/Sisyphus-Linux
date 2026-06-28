#!/usr/bin/env python3
"""Create a logind session for forge PID 1 (pam_systemd equivalent)."""
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
        return 1, "/dev/tty1"
    if not tty.startswith("/"):
        tty = f"/dev/{tty}"
    base = tty.rsplit("/", 1)[-1]
    if base.startswith("tty") and base[3:].isdigit():
        return int(base[3:]), tty
    return 1, tty


def _session_leader_pid() -> tuple[int, str]:
    for key in ("PAM_SESSION_PID", "XDG_SESSION_PID", "PAM_PARENT_PID"):
        raw = os.environ.get(key, "")
        if raw.isdigit():
            pid = int(raw)
            if pid > 1:
                return pid, key
    ppid = os.getppid()
    if ppid > 1:
        return ppid, "ppid"
    return os.getpid(), "self"


def _emit_pam_env(session_id: str, runtime: str, seat: str, vtnr: int, tty: str) -> None:
    # pam_exec with stdout imports VAR=VALUE lines into the PAM environment.
    print(f"XDG_SESSION_ID={session_id}")
    if runtime:
        print(f"XDG_RUNTIME_DIR={runtime}")
    if seat:
        print(f"XDG_SEAT={seat}")
    if vtnr > 0:
        print(f"XDG_VTNR={vtnr}")
    if tty:
        print(f"XDG_TTY={tty}")


def _find_session_id(iface, user: str, seat: str) -> str | None:
    for sid in iface.ListSessions():
        if len(sid) < 4:
            continue
        sid_id, _sid_uid, sid_user, sid_seat = sid[0], sid[1], sid[2], sid[3]
        if str(sid_user) != user:
            continue
        if str(sid_seat) not in (seat, ""):
            continue
        return str(sid_id)
    return None


def main() -> int:
    try:
        import dbus
    except ImportError:
        _log("python3-dbus missing")
        return 0

    user = os.environ.get("PAM_USER", "")
    uid = int(os.environ.get("PAM_UID", "0") or "0")
    service = os.environ.get("PAM_SERVICE", "login")
    session_type = os.environ.get("XDG_SESSION_TYPE", "wayland")
    session_class = os.environ.get("XDG_SESSION_CLASS", "greeter")
    seat = os.environ.get("XDG_SEAT", "seat0")
    desktop = os.environ.get("XDG_SESSION_DESKTOP", "COSMIC")
    vtnr, tty = _vtnr_and_tty()
    leader, leader_src = _session_leader_pid()
    bus_addr = os.environ.get(
        "DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/run/dbus/system_bus_socket"
    )

    iface = None
    try:
        bus = dbus.SystemBus(private=bus_addr)
        bus.set_exit_on_disconnect(False)
        mgr = bus.get_object(
            "org.freedesktop.login1", "/org/freedesktop/login1"
        )
        iface = dbus.Interface(
            mgr, "org.freedesktop.login1.Manager"
        )
        existing = _find_session_id(iface, user, seat)
        if existing:
            _emit_pam_env(existing, f"/run/user/{uid}", seat, vtnr, tty)
            _log(f"CreateSession reused existing session user={user} id={existing}")
            return 0
        result = iface.CreateSession(
            dbus.UInt32(uid),
            dbus.UInt32(leader),
            service,
            session_type,
            session_class,
            desktop,
            seat,
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
            f"CreateSession ok user={user} seat={seat} class={session_class} "
            f"type={session_type} vtnr={vtnr} tty={tty} leader={leader}({leader_src}) "
            f"id={session_id} runtime={runtime}"
        )
        _emit_pam_env(str(session_id), str(runtime), seat, vtnr, tty)
        try:
            iface.ActivateSession(str(session_id))
        except Exception as exc:  # noqa: BLE001
            _log(f"ActivateSession {session_id}: {exc!r}")
    except Exception as exc:  # noqa: BLE001
        msg = str(exc)
        if "Already running" in msg or "already exists" in msg.lower():
            existing = _find_session_id(iface, user, seat) if iface is not None else None
            if existing:
                _emit_pam_env(existing, f"/run/user/{uid}", seat, vtnr, tty)
                _log(f"CreateSession reused existing session user={user} id={existing}")
            else:
                _log(f"CreateSession skipped (existing session) user={user}")
            return 0
        _log(
            f"CreateSession failed user={user} seat={seat} vtnr={vtnr} tty={tty}: {exc!r}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())