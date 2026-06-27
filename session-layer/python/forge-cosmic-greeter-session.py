#!/usr/bin/env python3
"""Launch cosmic-greeter with a proper logind session under Forge PID 1."""
import os
import sys
import time

try:
    import dbus
except ImportError:
    dbus = None  # type: ignore[assignment]


def _log(msg: str) -> None:
    try:
        with open("/var/log/forge/cosmic-greeter.log", "a", encoding="utf-8") as fh:
            fh.write(msg + "\n")
    except OSError:
        pass


def _wait_for_name(bus, name: str, timeout: float = 15.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            if bus.name_has_owner(name):
                return True
        except Exception:  # noqa: BLE001
            pass
        time.sleep(0.1)
    return False


def _terminate_stale_sessions(iface, user: str) -> None:
    for entry in iface.ListSessions():
        if len(entry) < 5:
            continue
        sid, _uid, uname, seat, _path = entry
        if str(uname) != user or str(seat) != "seat0":
            continue
        try:
            iface.TerminateSession(str(sid))
            _log(f"terminated stale {user} session {sid}")
        except Exception as exc:  # noqa: BLE001
            _log(f"TerminateSession {sid}: {exc!r}")


def _session_env(runtime: str, bus_addr: str, vtnr: int, pw_dir: str) -> dict[str, str]:
    return {
        "HOME": pw_dir,
        "XDG_CONFIG_HOME": f"{pw_dir}/.config",
        "XDG_STATE_HOME": f"{pw_dir}/.local/state",
        "XDG_DATA_HOME": f"{pw_dir}/.local/share",
        "XDG_RUNTIME_DIR": runtime,
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={runtime}/bus",
        "DBUS_SYSTEM_BUS_ADDRESS": bus_addr,
        "XDG_SESSION_TYPE": "wayland",
        "XDG_SESSION_CLASS": "greeter",
        "XDG_CURRENT_DESKTOP": "COSMIC",
        "XDG_SEAT": "seat0",
        "XDG_VTNR": str(vtnr),
        "GDK_BACKEND": "wayland",
    }


def _start_user_session_bus(bus, uid: int, runtime: str) -> bool:
    if dbus is None:
        _log("forge-cosmic-greeter-session: python3-dbus missing")
        return False
    try:
        mgr = bus.get_object(
            "org.freedesktop.systemd1", "/org/freedesktop/systemd1"
        )
        systemd = dbus.Interface(mgr, "org.freedesktop.systemd1.Manager")
        systemd.StartUnit(f"user@{uid}.service", "replace")
    except Exception as exc:  # noqa: BLE001
        _log(f"StartUnit user@{uid}.service: {exc!r}")
        return False

    bus_path = f"{runtime}/bus"
    for _ in range(150):
        if os.path.exists(bus_path):
            return True
        time.sleep(0.1)
    _log(f"session bus missing at {bus_path}")
    return False


def main() -> int:
    user = "cosmic-greeter"
    vtnr = 1
    tty = f"/dev/tty{vtnr}"
    bus_addr = os.environ.get(
        "DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/run/dbus/system_bus_socket"
    )

    try:
        import pwd
    except ImportError as exc:
        _log(f"forge-cosmic-greeter-session: missing module: {exc!r}")
        return 1
    if dbus is None:
        _log("forge-cosmic-greeter-session: python3-dbus missing")
        return 1

    try:
        pw = pwd.getpwnam(user)
    except KeyError:
        _log("forge-cosmic-greeter-session: cosmic-greeter user missing")
        return 1

    uid = pw.pw_uid
    runtime = f"/run/user/{uid}"
    os.makedirs(runtime, mode=0o700, exist_ok=True)
    try:
        os.chown(runtime, uid, pw.pw_gid)
    except OSError:
        pass

    bus = dbus.SystemBus(private=bus_addr)
    bus.set_exit_on_disconnect(False)

    if not _wait_for_name(bus, "org.freedesktop.login1"):
        _log("forge-cosmic-greeter-session: logind not ready")
        return 1
    if not _wait_for_name(bus, "org.freedesktop.systemd1"):
        _log("forge-cosmic-greeter-session: systemd1 not ready")
        return 1

    mgr = bus.get_object("org.freedesktop.login1", "/org/freedesktop/login1")
    logind = dbus.Interface(mgr, "org.freedesktop.login1.Manager")
    _terminate_stale_sessions(logind, user)

    if not _start_user_session_bus(bus, uid, runtime):
        return 1

    sync_r, sync_w = os.pipe()
    child = os.fork()
    if child == 0:
        os.close(sync_w)
        os.read(sync_r, 1)
        os.close(sync_r)
        os.setsid()
        os.setgid(pw.pw_gid)
        os.initgroups(user, pw.pw_gid)
        os.setuid(uid)
        os.chdir(pw.pw_dir)
        # Clean stale sockets in our own runtime dir (leftover from previous crash in restart loop)
        try:
            for name in os.listdir(runtime):
                if name.startswith('wayland-') or name.startswith('.X') or 'X' in name:
                    p = os.path.join(runtime, name)
                    try:
                        if os.path.isdir(p):
                            import shutil
                            shutil.rmtree(p, ignore_errors=True)
                        else:
                            os.unlink(p)
                    except Exception:
                        pass
        except Exception:
            pass
        env = _session_env(runtime, bus_addr, vtnr, pw.pw_dir)
        env["PATH"] = os.environ.get("PATH", "/usr/bin:/bin")
        env["LANG"] = os.environ.get("LANG", "en_US.UTF-8")
        env["LC_ALL"] = os.environ.get("LC_ALL", env["LANG"])
        os.execvpe("/usr/bin/cosmic-greeter-start", ["/usr/bin/cosmic-greeter-start"], env)
        os._exit(127)

    if child < 0:
        _log("forge-cosmic-greeter-session: fork failed")
        return 1

    os.close(sync_r)
    try:
        result = logind.CreateSession(
            dbus.UInt32(uid),
            dbus.UInt32(child),
            "cosmic-greeter",
            "wayland",
            "greeter",
            "COSMIC",
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
            f"CreateSession ok user={user} leader={child} id={session_id} runtime={runtime}"
        )
        try:
            logind.ActivateSession(session_id)
            logind.ActivateSessionOnSeat(session_id, "seat0")
        except dbus.exceptions.DBusException as exc:
            _log(f"ActivateSession {session_id}: {exc!r}")
    except Exception as exc:  # noqa: BLE001
        _log(f"CreateSession failed user={user}: {exc!r}")
        try:
            os.kill(child, 15)
        except OSError:
            pass
        _, status = os.waitpid(child, 0)
        if os.WIFEXITED(status):
            return os.WEXITSTATUS(status)
        return 1
    finally:
        try:
            os.write(sync_w, b"\0")
        except OSError:
            pass
        os.close(sync_w)

    if os.path.exists(tty):
        os.system(f"chvt {vtnr} >/dev/null 2>&1")

    _, status = os.waitpid(child, 0)
    if os.WIFEXITED(status):
        code = os.WEXITSTATUS(status)
        _log(f"cosmic-greeter exited with status {code}")
        return code
    _log("cosmic-greeter terminated by signal")
    return 1


if __name__ == "__main__":
    sys.exit(main())