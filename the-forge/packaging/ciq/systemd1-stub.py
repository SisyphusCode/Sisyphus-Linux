#!/usr/bin/env python3
# Minimal org.freedesktop.systemd1 for forge PID 1 — logind/GDM session scopes.
import os
import pwd
import subprocess
import sys
import time

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop

BUS = os.environ.get(
    "FORGE_DBUS_SYSTEM_BUS",
    os.environ.get(
        "DBUS_SYSTEM_BUS_ADDRESS",
        "unix:path=/run/dbus/system_bus_socket",
    ),
)
MANAGER_IFACE = "org.freedesktop.systemd1.Manager"
SCOPE_IFACE = "org.freedesktop.systemd1.Scope"
PROPERTIES_IFACE = "org.freedesktop.DBus.Properties"
MANAGER_PATH = "/org/freedesktop/systemd1"
JOB_PATH = "/org/freedesktop/systemd1/job/forge"
FORGE_GREETER_ENV = "/run/gdm/forge-greeter-environment"
DEFAULT_ENV = (
    "WLR_NO_HARDWARE_CURSORS=1",
    "__GLX_VENDOR_LIBRARY_NAME=nvidia",
    "GBM_BACKEND=nvidia-drm",
)
DBUS_UNIX_FD = getattr(dbus, "UnixFd", None)


def _is_unix_fd(value) -> bool:
    if DBUS_UNIX_FD is not None and isinstance(value, DBUS_UNIX_FD):
        return True
    return hasattr(value, "take") and callable(getattr(value, "take", None))


def _greeter_environment() -> list[str]:
    entries: list[str] = []
    if os.path.isfile(FORGE_GREETER_ENV):
        try:
            with open(FORGE_GREETER_ENV, encoding="utf-8") as fh:
                for line in fh:
                    line = line.strip()
                    if line and "=" in line and not line.startswith("#"):
                        entries.append(line)
        except OSError:
            pass
    return entries or list(DEFAULT_ENV)


from gi.repository import GLib


def _unit_object_path(name: str) -> str:
    # systemd bus path encoding (see bus_path_encode_unique in systemd).
    encoded: list[str] = []
    for ch in name:
        if ch == "-":
            encoded.append("_2d")
        elif ch == ".":
            encoded.append("_2e")
        elif ch == "_":
            encoded.append("_5f")
        else:
            encoded.append(ch)
    return f"/org/freedesktop/systemd1/unit/{''.join(encoded)}"


class ForgeScope(dbus.service.Object):
    """Minimal scope object — logind calls Abandon when reusing session IDs."""

    def __init__(self, bus, unit_name: str):
        self.unit_name = unit_name
        super().__init__(bus, _unit_object_path(unit_name))

    @dbus.service.method(SCOPE_IFACE, in_signature="", out_signature="")
    def Abandon(self):
        _log(f"{self.unit_name}: Scope.Abandon")
        return None


class ForgeSystemd1(dbus.service.Object):
    def __init__(self, bus):
        self.bus = bus
        self._scopes: dict[str, ForgeScope] = {}
        self._pid_units: dict[int, str] = {}
        reply = bus.request_name("org.freedesktop.systemd1", dbus.bus.NAME_FLAG_DO_NOT_QUEUE)
        if reply == dbus.bus.REQUEST_NAME_REPLY_EXISTS:
            _log("org.freedesktop.systemd1 already owned — exiting activation helper")
            raise SystemExit(0)
        if reply != dbus.bus.REQUEST_NAME_REPLY_PRIMARY_OWNER:
            raise RuntimeError(f"request_name org.freedesktop.systemd1 failed: reply={reply}")
        super().__init__(bus, MANAGER_PATH)

    @dbus.service.signal(MANAGER_IFACE, signature="uoss")
    def JobRemoved(self, id, job, unit, result):
        _log(f"JobRemoved signal sent for {unit}")

    @dbus.service.method(MANAGER_IFACE, in_signature="ss", out_signature="o")
    def StartUnit(self, name, mode):
        self._handle_unit(name)
        def emit_done():
            self.JobRemoved(dbus.UInt32(1), dbus.ObjectPath(JOB_PATH), name, "done")
            return False
        GLib.timeout_add(50, emit_done)
        return dbus.ObjectPath(JOB_PATH)

    @dbus.service.method(MANAGER_IFACE, in_signature="ssa(sv)a(sa(sv))", out_signature="o")
    def StartTransientUnit(self, name, mode, properties, aux):
        if name.startswith("session-") and name.endswith(".scope"):
            self._handle_session_scope(name, properties, aux)
        else:
            self._handle_unit(name)
        def emit_done():
            self.JobRemoved(dbus.UInt32(1), dbus.ObjectPath(JOB_PATH), name, "done")
            return False
        GLib.timeout_add(50, emit_done)
        return dbus.ObjectPath(JOB_PATH)

    @dbus.service.method(MANAGER_IFACE, in_signature="ss", out_signature="o")
    def StopUnit(self, name, mode):
        return JOB_PATH

    @dbus.service.method(MANAGER_IFACE, in_signature="", out_signature="")
    def Subscribe(self):
        return None

    @dbus.service.method(MANAGER_IFACE, in_signature="u", out_signature="o")
    def GetUnitByPID(self, pid):
        unit = self._unit_for_pid(int(pid))
        if unit:
            self._ensure_scope(unit)
            return _unit_object_path(unit)
        return "/org/freedesktop/systemd1/unit/_forge_scope"

    @dbus.service.method(PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, interface_name, property_name):
        if interface_name == MANAGER_IFACE and property_name == "Environment":
            return dbus.Array(_greeter_environment(), signature="s")
        raise dbus.exceptions.DBusException(
            f"org.freedesktop.DBus.Error.UnknownProperty: Unknown property {property_name}"
        )

    def _handle_unit(self, name: str) -> None:
        if name.startswith("user@") and name.endswith(".service"):
            uid = name.split("@", 1)[1].split(".", 1)[0]
            self._start_user_manager(uid)
    def _ensure_scope(self, name: str) -> ForgeScope:
        scope = self._scopes.get(name)
        if scope is None:
            scope = ForgeScope(self.bus, name)
            self._scopes[name] = scope
        return scope

    def _handle_session_scope(self, name: str, properties, aux=None) -> None:
        self._ensure_scope(name)
        leader = _leader_from_properties(properties)
        if leader is None and aux:
            for aux_unit in aux or []:
                if len(aux_unit) < 2:
                    continue
                leader = _leader_from_properties(aux_unit[1])
                if leader is not None:
                    break
        if leader is None:
            _log(
                f"{name}: no leader PID in StartTransientUnit "
                f"properties={properties!r} aux={aux!r}"
            )
            return
        slice_name = _property_string(properties, "Slice") or "system.slice"
        cgroup = os.path.join("/sys/fs/cgroup", _slice_cgroup_path(slice_name), name)
        try:
            os.makedirs(cgroup, exist_ok=True)
            with open(f"{cgroup}/cgroup.procs", "w", encoding="ascii") as fh:
                fh.write(f"{leader}\n")
            self._pid_units[leader] = name
            _log(f"{name}: attached leader pid {leader} to cgroup {cgroup}")
        except OSError as exc:
            _log(f"{name}: cgroup attach failed for pid {leader}: {exc!r}")

    def _unit_for_pid(self, pid: int) -> str | None:
        mapped = self._pid_units.get(pid)
        if mapped:
            return mapped
        try:
            with open(f"/proc/{pid}/cgroup", encoding="ascii") as fh:
                for line in fh:
                    parts = line.strip().split(":", 2)
                    if len(parts) != 3:
                        continue
                    for component in reversed(parts[2].split("/")):
                        if component.startswith("session-") and component.endswith(".scope"):
                            return component
        except OSError:
            return None
        return None

    def _start_user_manager(self, uid: str) -> None:
        try:
            uid_i = int(uid)
            pw = pwd.getpwuid(uid_i)
        except (ValueError, KeyError):
            return

        runtime = f"/run/user/{uid}"
        bus_path = f"{runtime}/bus"
        os.makedirs(runtime, mode=0o700, exist_ok=True)
        try:
            os.chown(runtime, uid_i, pw.pw_gid)
        except OSError:
            pass

        if os.path.exists(bus_path):
            _log(f"user@{uid}.service: session bus already at {bus_path}")
            return

        env = os.environ.copy()
        env["XDG_RUNTIME_DIR"] = runtime
        env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={bus_path}"
        env["HOME"] = pw.pw_dir
        env["USER"] = pw.pw_name
        env["LOGNAME"] = pw.pw_name

        def drop_privs():
            try:
                os.initgroups(pw.pw_name, pw.pw_gid)
                os.setgid(pw.pw_gid)
                os.setuid(uid_i)
            except OSError:
                pass

        # systemd --user refuses to run when PID 1 is not systemd; provide a session bus stub instead.
        # Drop privileges cleanly natively in Python to preserve the environment variables
        subprocess.Popen(
            [
                "/usr/bin/dbus-daemon",
                "--session",
                f"--address=unix:path={bus_path}",
                "--nopidfile",
                "--nofork",
            ],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            cwd=pw.pw_dir,
            preexec_fn=drop_privs,
        )
        for _ in range(50):
            if os.path.exists(bus_path):
                break
            time.sleep(0.1)
        try:
            os.chown(bus_path, uid_i, pw.pw_gid)
        except OSError:
            pass

        subprocess.Popen(
            ["/usr/libexec/forge/systemd1-session-stub-wrapper.sh"],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            cwd=pw.pw_dir,
            preexec_fn=drop_privs,
        )
        
        # Start pipewire and wireplumber to satisfy cosmic-greeter and cosmic-comp audio dependencies
        subprocess.Popen(
            ["/usr/bin/pipewire"],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            cwd=pw.pw_dir,
            preexec_fn=drop_privs,
        )
        subprocess.Popen(
            ["/usr/bin/wireplumber"],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            cwd=pw.pw_dir,
            preexec_fn=drop_privs,
        )
        _log(f"user@{uid}.service: started session bus + audio + stub at {bus_path}")


def _dbus_scalar(value):
    if value is None:
        return None
    if _is_unix_fd(value):
        # Never coerce UnixFd through int() here; PIDFD ownership is handled explicitly.
        return value
    if isinstance(value, (dbus.String, str)):
        return str(value)
    if isinstance(value, (dbus.Int32, dbus.Int64, dbus.UInt32, dbus.UInt64, int)):
        return int(value)
    if isinstance(value, (dbus.Array, list, tuple)):
        return [_dbus_scalar(item) for item in value]
    if isinstance(value, dbus.Struct):
        return tuple(_dbus_scalar(item) for item in value)
    try:
        return int(value)
    except (TypeError, ValueError):
        return value


def _pid_from_pidfd(value):
    fd = None
    try:
        if _is_unix_fd(value):
            fd = value.take()
        else:
            fd = int(value)
    except (TypeError, ValueError, AttributeError):
        return None

    try:
        with open(f"/proc/self/fdinfo/{fd}", encoding="utf-8") as fh:
            for line in fh:
                if line.startswith("Pid:"):
                    return int(line.split(":", 1)[1].strip())
    except (OSError, ValueError):
        return None
    finally:
        if fd is not None:
            try:
                os.close(fd)
            except OSError:
                pass
    return None


def _property_string(properties, name: str) -> str | None:
    for entry in properties or []:
        if len(entry) < 2 or str(entry[0]) != name:
            continue
        value = _dbus_scalar(entry[1])
        return str(value) if value not in (None, "") else None
    return None


def _slice_cgroup_path(slice_name: str) -> str:
    if not slice_name.endswith(".slice"):
        return slice_name
    stem = slice_name[:-6]
    if not stem:
        return slice_name
    parts = stem.split("-")
    if len(parts) == 1:
        return slice_name
    return "/".join(f"{'-'.join(parts[:index])}.slice" for index in range(1, len(parts) + 1))


def _leader_from_properties(properties):
    leader = None
    pids: list[int] = []
    pidfds: list[int] = []
    for entry in properties or []:
        if len(entry) < 2:
            continue
        key = str(entry[0])
        raw_value = entry[1]
        if key in ("PIDFDs", "PIDFD", "pidfds", "pidfd"):
            fd_values = raw_value if isinstance(raw_value, (dbus.Array, list, tuple)) else [raw_value]
            for fdv in fd_values:
                pid = _pid_from_pidfd(fdv)
                if pid is not None:
                    pidfds.append(pid)
            continue

        value = _dbus_scalar(raw_value)
        if key in ("Leader", "MainPID", "main-pid"):
            try:
                leader = int(value)
            except (TypeError, ValueError):
                pass
        elif key in ("PIDs", "PID"):
            if isinstance(value, list):
                for pid in value:
                    try:
                        pids.append(int(pid))
                    except (TypeError, ValueError):
                        pass
            else:
                try:
                    pids.append(int(value))
                except (TypeError, ValueError):
                    pass
    result = leader or (pidfds[0] if pidfds else None) or (pids[0] if pids else None)
    if result is None:
        _log(f"StartTransientUnit: no leader in properties={properties!r}")
    return result


def _log(msg: str) -> None:
    path = "/var/log/forge/systemd1-stub.log"
    try:
        with open(path, "a", encoding="utf-8") as f:
            f.write(msg + "\n")
    except OSError:
        pass


def main() -> int:
    try:
        DBusGMainLoop(set_as_default=True)
        addr = os.environ.get("DBUS_SYSTEM_BUS_ADDRESS") or BUS
        _log(f"connecting to {addr}")
        bus = dbus.SystemBus(private=addr)
        manager = ForgeSystemd1(bus)
        # logind may query Abandon on a pre-existing scope before StartTransientUnit.
        manager._ensure_scope("session-c1.scope")
        _log("org.freedesktop.systemd1 name acquired")
        from gi.repository import GLib

        GLib.MainLoop().run()
        return 0
    except Exception as exc:  # noqa: BLE001 — log all startup failures for PID 1 debug
        _log(f"FATAL: {exc!r}")
        raise


if __name__ == "__main__":
    sys.exit(main())