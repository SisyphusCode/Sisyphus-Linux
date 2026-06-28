#!/usr/bin/env python3
"""Minimal org.freedesktop.locale1 provider for Forge live sessions."""
import os
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

BUS = os.environ.get("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/run/dbus/system_bus_socket")
IFACE = "org.freedesktop.locale1"
PROPERTIES_IFACE = "org.freedesktop.DBus.Properties"
PATH = "/org/freedesktop/locale1"


def _log(message: str) -> None:
    try:
        os.makedirs("/var/log/forge", exist_ok=True)
        with open("/var/log/forge/localed-stub.log", "a", encoding="utf-8") as fh:
            fh.write(message + "\n")
    except OSError:
        pass


class Locale1(dbus.service.Object):
    def __init__(self, bus):
        self.locale = dbus.Array(["LANG=en_US.UTF-8"], signature="s")
        self.vconsole_keymap = "us"
        self.vconsole_keymap_toggle = ""
        self.x11_layout = "us"
        self.x11_model = ""
        self.x11_variant = ""
        self.x11_options = ""
        reply = bus.request_name(IFACE, dbus.bus.NAME_FLAG_DO_NOT_QUEUE)
        if reply == dbus.bus.REQUEST_NAME_REPLY_EXISTS:
            _log("org.freedesktop.locale1 already owned; exiting")
            raise SystemExit(0)
        if reply != dbus.bus.REQUEST_NAME_REPLY_PRIMARY_OWNER:
            raise RuntimeError(f"request_name {IFACE} failed: reply={reply}")
        super().__init__(bus, PATH)

    @dbus.service.method(IFACE, in_signature="asb", out_signature="")
    def SetLocale(self, locale, interactive):
        self.locale = dbus.Array([str(item) for item in locale], signature="s")
        _log(f"SetLocale locale={list(self.locale)!r} interactive={bool(interactive)}")

    @dbus.service.method(IFACE, in_signature="ssbb", out_signature="")
    def SetVConsoleKeyboard(self, keymap, keymap_toggle, convert, interactive):
        self.vconsole_keymap = str(keymap)
        self.vconsole_keymap_toggle = str(keymap_toggle)
        _log(
            "SetVConsoleKeyboard "
            f"keymap={self.vconsole_keymap!r} toggle={self.vconsole_keymap_toggle!r} "
            f"convert={bool(convert)} interactive={bool(interactive)}"
        )

    @dbus.service.method(IFACE, in_signature="ssssbb", out_signature="")
    def SetX11Keyboard(self, layout, model, variant, options, convert, interactive):
        self.x11_layout = str(layout)
        self.x11_model = str(model)
        self.x11_variant = str(variant)
        self.x11_options = str(options)
        _log(
            "SetX11Keyboard "
            f"layout={self.x11_layout!r} model={self.x11_model!r} "
            f"variant={self.x11_variant!r} options={self.x11_options!r} "
            f"convert={bool(convert)} interactive={bool(interactive)}"
        )

    @dbus.service.method(PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, interface_name, property_name):
        return self.GetAll(interface_name)[property_name]

    @dbus.service.method(PROPERTIES_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface_name):
        if interface_name != IFACE:
            return dbus.Dictionary({}, signature="sv")
        return dbus.Dictionary(
            {
                "Locale": self.locale,
                "VConsoleKeymap": dbus.String(self.vconsole_keymap),
                "VConsoleKeymapToggle": dbus.String(self.vconsole_keymap_toggle),
                "X11Layout": dbus.String(self.x11_layout),
                "X11Model": dbus.String(self.x11_model),
                "X11Variant": dbus.String(self.x11_variant),
                "X11Options": dbus.String(self.x11_options),
            },
            signature="sv",
        )

    @dbus.service.method(PROPERTIES_IFACE, in_signature="ssv", out_signature="")
    def Set(self, interface_name, property_name, value):
        raise dbus.exceptions.DBusException(
            "org.freedesktop.DBus.Error.PropertyReadOnly",
            f"{property_name} is read-only",
        )


def main() -> int:
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus(private=BUS)
    Locale1(bus)
    _log("org.freedesktop.locale1 name acquired")
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
