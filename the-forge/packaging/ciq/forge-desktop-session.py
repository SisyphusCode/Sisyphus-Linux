#!/usr/bin/env python3
"""Open a graphical logind session and launch gdm-x-session as session leader."""
import os
import sys


def _log(msg: str) -> None:
    try:
        with open("/var/log/forge/forge-desktop.log", "a", encoding="utf-8") as fh:
            fh.write(msg + "\n")
    except OSError:
        pass


def _terminate_stale_gdm_sessions(iface) -> None:
    for entry in iface.ListSessions():
        if len(entry) < 5:
            continue
        sid, _uid, uname, seat, _path = entry
        if str(uname) != "gdm" or str(seat) != "seat0":
            continue
        try:
            iface.TerminateSession(str(sid))
            _log(f"terminated stale gdm session {sid}")
        except Exception as exc:  # noqa: BLE001
            _log(f"TerminateSession {sid}: {exc!r}")


def _session_env(runtime: str, bus_addr: str, vtnr: int) -> dict[str, str]:
    return {
        "XDG_RUNTIME_DIR": runtime,
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={runtime}/bus",
        "DBUS_SYSTEM_BUS_ADDRESS": bus_addr,
        "XDG_SESSION_TYPE": "x11",
        "XDG_SESSION_CLASS": "user",
        "XDG_SESSION_DESKTOP": "gnome",
        "XDG_CURRENT_DESKTOP": "GNOME",
        "XDG_VTNR": str(vtnr),
        "GDK_BACKEND": "x11",
    }


def main() -> int:
    if len(sys.argv) < 4:
        print("usage: forge-desktop-session.py UID USERNAME VT", file=sys.stderr)
        return 2

    uid = int(sys.argv[1])
    user = sys.argv[2]
    vtnr = int(sys.argv[3])
    tty = f"/dev/tty{vtnr}"
    bus_addr = os.environ.get(
        "DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/run/dbus/system_bus_socket"
    )
    runtime = f"/run/user/{uid}"
    xsession = "/usr/libexec/gdm-x-session"

    try:
        import dbus
    except ImportError:
        _log("forge-desktop-session: python3-dbus missing")
        return 1

    if not os.path.isfile(xsession) or not os.access(xsession, os.X_OK):
        _log(f"forge-desktop-session: {xsession} not found")
        return 127

    try:
        bus = dbus.SystemBus(private=bus_addr)
        bus.set_exit_on_disconnect(False)
        mgr = bus.get_object(
            "org.freedesktop.login1", "/org/freedesktop/login1"
        )
        iface = dbus.Interface(mgr, "org.freedesktop.login1.Manager")

        _terminate_stale_gdm_sessions(iface)

        sync_r, sync_w = os.pipe()
        child = os.fork()
        if child == 0:
            os.close(sync_w)
            os.read(sync_r, 1)
            os.close(sync_r)
            env = os.environ.copy()
            env.update(_session_env(runtime, bus_addr, vtnr))
            os.execvpe(
                "runuser",
                [
                    "runuser",
                    "-u",
                    user,
                    "--",
                    "env",
                    *[
                        f"{key}={value}"
                        for key, value in _session_env(runtime, bus_addr, vtnr).items()
                    ],
                    xsession,
                    "--register-session",
                    "--run-script",
                    "gnome-session",
                ],
                env,
            )
            os._exit(127)

        if child < 0:
            _log("forge-desktop-session: fork failed")
            return 1

        os.close(sync_r)
        try:
            result = iface.CreateSession(
                dbus.UInt32(uid),
                dbus.UInt32(child),
                "forge-desktop",
                "x11",
                "user",
                "GNOME",
                "seat0",
                dbus.UInt32(vtnr),
                tty,
                "",
                dbus.Boolean(False),
                "",
                "",
                dbus.Array([], signature="(sv)"),
            )
            session_id = str(result[0])
            _log(
                f"CreateSession x11 user={user} leader={child} "
                f"seat0 vtnr={vtnr} id={session_id}"
            )
            if os.path.exists(f"/dev/tty{vtnr}"):
                try:
                    os.system(f"chvt {vtnr} >/dev/null 2>&1")
                except OSError:
                    pass
            iface.ActivateSession(session_id)
            try:
                iface.ActivateSessionOnSeat(session_id, "seat0")
            except dbus.exceptions.DBusException:
                pass
        finally:
            os.write(sync_w, b"\0")
            os.close(sync_w)

        _, status = os.waitpid(child, 0)
        if os.WIFEXITED(status):
            return os.WEXITSTATUS(status)
        if os.WIFSIGNALED(status):
            return 128 + os.WTERMSIG(status)
        return 1
    except Exception as exc:  # noqa: BLE001
        _log(f"forge-desktop-session failed: {exc!r}")
        return 1


if __name__ == "__main__":
    sys.exit(main())