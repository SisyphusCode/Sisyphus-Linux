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
    # systemd unit paths: session-c1.scope -> /org/freedesktop/systemd1/unit/session_c1_scope
    slug = name.replace("-", "_").replace(".", "_")
    return f"/org/freedesktop/systemd1/unit/{slug}"


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
            self.JobRemoved(1, JOB_PATH, name, "done")
            return False
        GLib.timeout_add(50, emit_done)
        return JOB_PATH

    @dbus.service.method(MANAGER_IFACE, in_signature="ssa(sv)a(sa(sv))", out_signature="o")
    def StartTransientUnit(self, name, mode, properties, aux):
        if name.startswith("session-") and name.endswith(".scope"):
            self._handle_session_scope(name, properties)
        else:
            self._handle_unit(name)
        def emit_done():
            self.JobRemoved(1, JOB_PATH, name, "done")
            return False
        GLib.timeout_add(50, emit_done)
        return JOB_PATH

    @dbus.service.method(MANAGER_IFACE, in_signature="ss", out_signature="o")
    def StopUnit(self, name, mode):
        return JOB_PATH

    @dbus.service.method(MANAGER_IFACE, in_signature="", out_signature="")
    def Subscribe(self):
        return None

    @dbus.service.method(MANAGER_IFACE, in_signature="u", out_signature="o")
    def GetUnitByPID(self, pid):
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

    def _handle_session_scope(self, name: str, properties) -> None:
        self._ensure_scope(name)
        leader = _leader_from_properties(properties)
        if leader is None:
            _log(f"{name}: no leader PID in StartTransientUnit properties")
            return
        cgroup = f"/sys/fs/cgroup/{name}"
        try:
            os.makedirs(cgroup, exist_ok=True)
            with open(f"{cgroup}/cgroup.procs", "w", encoding="ascii") as fh:
                fh.write(f"{leader}\n")
            _log(f"{name}: attached leader pid {leader} to cgroup")
        except OSError as exc:
            _log(f"{name}: cgroup attach failed for pid {leader}: {exc!r}")

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

        # systemd --user refuses to run when PID 1 is not systemd; provide a session bus stub instead.
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
        )
        _log(f"user@{uid}.service: started session bus + stub at {bus_path}")


def _leader_from_properties(properties):
    leader = None
    pids: list[int] = []
    for entry in properties or []:
        if len(entry) < 2:
            continue
        key = str(entry[0])
        value = entry[1]
        if key == "Leader":
            try:
                leader = int(value)
            except (TypeError, ValueError):
                pass
        elif key == "PIDs":
            try:
                pids = [int(pid) for pid in value]
            except (TypeError, ValueError):
                pass
    return leader or (pids[0] if pids else None)


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