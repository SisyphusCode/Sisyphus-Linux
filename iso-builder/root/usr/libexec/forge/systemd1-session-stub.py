#!/usr/bin/env python3
# Session-bus org.freedesktop.systemd1 for forge PID 1 (gdm-wayland-session import_environment).
import os
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop

MANAGER_IFACE = "org.freedesktop.systemd1.Manager"
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
    path = FORGE_GREETER_ENV
    if os.path.isfile(path):
        try:
            with open(path, encoding="utf-8") as fh:
                for line in fh:
                    line = line.strip()
                    if line and "=" in line and not line.startswith("#"):
                        entries.append(line)
        except OSError:
            pass
    if entries:
        return entries
    return list(DEFAULT_ENV)


from gi.repository import GLib

class ForgeSessionSystemd1(dbus.service.Object):
    def __init__(self, bus):
        self.bus = bus
        bus.request_name("org.freedesktop.systemd1", dbus.bus.NAME_FLAG_DO_NOT_QUEUE)
        super().__init__(bus, MANAGER_PATH)

    @dbus.service.signal(MANAGER_IFACE, signature="uoss")
    def JobRemoved(self, id, job, unit, result):
        _log(f"JobRemoved signal sent for {unit}")

    @dbus.service.method(MANAGER_IFACE, in_signature="ss", out_signature="o")
    def StartUnit(self, name, mode):
        def emit_done():
            self.JobRemoved(dbus.UInt32(1), dbus.ObjectPath(JOB_PATH), name, "done")
            return False
        GLib.timeout_add(50, emit_done)
        return dbus.ObjectPath(JOB_PATH)

    @dbus.service.method(MANAGER_IFACE, in_signature="ssa(sv)a(sa(sv))", out_signature="o")
    def StartTransientUnit(self, name, mode, properties, aux):
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
        return "/org/freedesktop/systemd1/unit/_fake"

    @dbus.service.method(PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, interface_name, property_name):
        if interface_name == MANAGER_IFACE and property_name == "Environment":
            return dbus.Array(_greeter_environment(), signature="s")
        raise dbus.exceptions.DBusException(
            f"org.freedesktop.DBus.Error.UnknownProperty: Unknown property {property_name}"
        )

    @dbus.service.method(
        MANAGER_IFACE, in_signature="as", out_signature="", sender_keyword="sender"
    )
    def SetEnvironment(self, variables, sender=None):
        return None

    @dbus.service.method(MANAGER_IFACE, in_signature="s", out_signature="")
    def UnsetEnvironment(self, variable):
        return None

    @dbus.service.method(MANAGER_IFACE, in_signature="", out_signature="")
    def ResetEnvironment(self):
        return None


def _log(msg: str) -> None:
    uid = os.getuid()
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{uid}"
    candidates = [
        f"{runtime}/systemd1-session-stub.log",
        f"/tmp/systemd1-session-stub-{uid}.log",
        "/var/log/forge/systemd1-session-stub.log",
    ]
    for path in candidates:
        try:
            with open(path, "a", encoding="utf-8") as f:
                f.write(msg + "\n")
            return
        except OSError:
            continue


def _connect_session_bus():
    starter = os.environ.get("DBUS_STARTER_ADDRESS", "")
    addr = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    if starter or os.environ.get("DBUS_STARTER_BUS_TYPE") == "session":
        return dbus.SessionBus()
    if addr.startswith("unix:"):
        return dbus.SessionBus(private=addr)
    return dbus.SessionBus()


def main() -> int:
    try:
        DBusGMainLoop(set_as_default=True)
        _log(
            "session stub starting "
            f"DBUS_SESSION_BUS_ADDRESS={os.environ.get('DBUS_SESSION_BUS_ADDRESS', 'unset')} "
            f"DBUS_STARTER_ADDRESS={os.environ.get('DBUS_STARTER_ADDRESS', 'unset')}"
        )
        bus = _connect_session_bus()
        ForgeSessionSystemd1(bus)
        _log("session org.freedesktop.systemd1 acquired")
        from gi.repository import GLib

        GLib.MainLoop().run()
        return 0
    except Exception as exc:  # noqa: BLE001
        _log(f"FATAL: {exc!r}")
        raise


if __name__ == "__main__":
    sys.exit(main())