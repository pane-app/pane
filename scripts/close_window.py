#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0 OR MIT
"""Asks an X11 window to close, as a window manager's close button does:
sends it a WM_PROTOCOLS client message with WM_DELETE_WINDOW. The
application then closes the window itself (Pane quits when its window
closes), unlike `xdotool windowclose`, which destroys the window from
outside. Uses only libX11, through ctypes, on the display in $DISPLAY.

Usage: close_window.py <window-id>
"""
import ctypes
import ctypes.util
import sys


class ClientMessage(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
        ("message_type", ctypes.c_ulong),
        ("format", ctypes.c_int),
        ("data", ctypes.c_long * 5),
    ]


class Event(ctypes.Union):
    # XEvent is 24 longs on every platform.
    _fields_ = [("client", ClientMessage), ("pad", ctypes.c_long * 24)]


CLIENT_MESSAGE = 33
CURRENT_TIME = 0
NO_EVENT_MASK = 0


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    window = int(sys.argv[1], 0)
    name = ctypes.util.find_library("X11") or "libX11.so.6"
    x11 = ctypes.CDLL(name)
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XInternAtom.restype = ctypes.c_ulong
    x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x11.XSendEvent.argtypes = [
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_int,
        ctypes.c_long,
        ctypes.POINTER(Event),
    ]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(None)
    if not display:
        print("close_window: cannot open the display", file=sys.stderr)
        return 1
    event = Event()
    event.client.type = CLIENT_MESSAGE
    event.client.window = window
    event.client.message_type = x11.XInternAtom(display, b"WM_PROTOCOLS", False)
    event.client.format = 32
    event.client.data[0] = x11.XInternAtom(display, b"WM_DELETE_WINDOW", False)
    event.client.data[1] = CURRENT_TIME
    sent = x11.XSendEvent(display, window, False, NO_EVENT_MASK, ctypes.byref(event))
    x11.XFlush(display)
    x11.XCloseDisplay(display)
    return 0 if sent else 1


if __name__ == "__main__":
    sys.exit(main())
