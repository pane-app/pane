#!/usr/bin/env python3
"""A stand-in freedesktop notification service for the Linux smoke (#559).

Owns org.freedesktop.Notifications on the session bus (the smoke's own,
from dbus-launch), shows nothing, and writes each notification it is asked
to show to LOG, one JSON object a line, with the id it answered. Once it
owns the name it writes LOG.ready. While it runs, a file LOG.click holding
a notification's id makes it emit ActionInvoked for that id's default
action, as a desktop does when the user clicks the notification, and then
removes the file.

Needs Python's GObject bindings (python3-gi), which the smoke needs anyway.

Usage: notification_service.py LOG
"""

import json
import os
import sys

from gi.repository import Gio, GLib

NAME = "org.freedesktop.Notifications"
PATH = "/org/freedesktop/Notifications"
INTERFACE = """
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="Notify">
      <arg type="s" direction="in"/><arg type="u" direction="in"/>
      <arg type="s" direction="in"/><arg type="s" direction="in"/>
      <arg type="s" direction="in"/><arg type="as" direction="in"/>
      <arg type="a{sv}" direction="in"/><arg type="i" direction="in"/>
      <arg type="u" direction="out"/>
    </method>
    <method name="GetCapabilities"><arg type="as" direction="out"/></method>
    <method name="GetServerInformation">
      <arg type="s" direction="out"/><arg type="s" direction="out"/>
      <arg type="s" direction="out"/><arg type="s" direction="out"/>
    </method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
    <signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
    <signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
  </interface>
</node>
"""


def main():
    log = sys.argv[1]
    shown = [0]
    connection = [None]

    def called(conn, sender, path, interface, method, parameters, invocation):
        if method == "Notify":
            app_name, _replaces, _icon, summary, body, actions, hints, _timeout = parameters.unpack()
            shown[0] += 1
            record = {
                "id": shown[0],
                "app_name": app_name,
                "summary": summary,
                "body": body,
                "actions": actions,
                "hints": {key: value for key, value in hints.items()
                          if isinstance(value, (bool, int, str))},
            }
            with open(log, "a", encoding="utf-8") as file:
                file.write(json.dumps(record) + "\n")
            invocation.return_value(GLib.Variant("(u)", (shown[0],)))
        elif method == "GetCapabilities":
            invocation.return_value(GLib.Variant("(as)", (["actions", "body"],)))
        elif method == "GetServerInformation":
            invocation.return_value(GLib.Variant("(ssss)", ("stand-in", "pane", "1", "1.2")))
        else:
            invocation.return_value(None)

    def acquired(conn, name):
        connection[0] = conn
        open(log + ".ready", "w").close()

    def bus_acquired(conn, name):
        info = Gio.DBusNodeInfo.new_for_xml(INTERFACE).interfaces[0]
        conn.register_object(PATH, info, called, None, None)

    def clicks():
        click = log + ".click"
        if connection[0] is not None and os.path.exists(click):
            with open(click, encoding="utf-8") as file:
                notification = int(file.read().strip())
            os.remove(click)
            connection[0].emit_signal(
                None, PATH, NAME, "ActionInvoked", GLib.Variant("(us)", (notification, "default")))
        return True

    Gio.bus_own_name(Gio.BusType.SESSION, NAME, Gio.BusNameOwnerFlags.NONE,
                     bus_acquired, acquired, None)
    GLib.timeout_add(100, clicks)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
