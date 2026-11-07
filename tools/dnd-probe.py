#!/usr/bin/env python3
"""Drag a file into Attic the way a file manager does, inside a private
headless mutter, and report what happened.

    tools/dnd-probe.py target/debug/attic "example photos/x.jpg" 800 500 1200 650

Starts a private session bus and `mutter --headless` (1600x1000), gives its
seat a pointer and a keyboard through mutter's RemoteDesktop API (the
clipboard library only creates a data device once the seat has a keyboard),
runs the app on Wayland with WAYLAND_DEBUG=1 and a scratch notes dir, then a
small GTK 4 window (tools/dnd-source.py) offering the file with copy / move /
link / ask, like Nautilus does, and which deletes the file if the drop was a
move. The virtual pointer presses on the source window (the 4th/5th args,
window centre by default) and drags to the 6th/7th (over the editor). Output:
the source's events, the app's wl_data_* protocol lines and attic log lines,
and the note's image lines. Nothing touches the real desktop.
"""
import os, subprocess, time, tempfile, sys, glob, shutil, signal
T0 = time.time()
def phase(msg):
    print(f"[{time.time()-T0:5.1f}s] {msg}", file=sys.stderr, flush=True)
# A SIGTERM from `timeout` must still run the cleanup below.
signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
from gi.repository import Gio, GLib
S = os.path.dirname(os.path.abspath(__file__))  # dnd-source.py lives beside this file
bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--print-address=1"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, start_new_session=True)
addr = bus.stdout.readline().decode().strip()
mlog = tempfile.NamedTemporaryFile(prefix="probe-mutter-", suffix=".log", delete=False)
name = f"probe-{os.getpid()}"
mutter = subprocess.Popen(["mutter", "--headless", "--wayland", "--virtual-monitor", "1600x1000", "--wayland-display", name],
                          env=dict(os.environ, DBUS_SESSION_BUS_ADDRESS=addr, MUTTER_DEBUG="wayland,dnd,input-events"), stdout=mlog, stderr=subprocess.STDOUT, start_new_session=True)
phase("mutter started")
app = src = None
try:
    for _ in range(100):
        time.sleep(0.1)
        if "Using Wayland display name" in open(mlog.name).read(): break
    phase("connecting to the private bus")
    conn = Gio.DBusConnection.new_for_address_sync(addr, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
    rd = "org.gnome.Mutter.RemoteDesktop"; sc = "org.gnome.Mutter.ScreenCast"
    def call(dest, path, iface, method, params=None):
        return conn.call_sync(dest, path, iface, method, params, None, Gio.DBusCallFlags.NONE, 5000, None)
    # mutter registers its RemoteDesktop service a little after it is up.
    for _ in range(100):
        owned = conn.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner",
                               GLib.Variant("(s)", (rd,)), None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
        if owned: break
        time.sleep(0.1)
    phase(f"remote desktop service owned={owned}")
    session = call(rd, "/org/gnome/Mutter/RemoteDesktop", rd, "CreateSession").unpack()[0]
    sid = conn.call_sync(rd, session, "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", (rd + ".Session", "SessionId")), None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
    cast = call(sc, "/org/gnome/Mutter/ScreenCast", sc, "CreateSession", GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", sid)},))).unpack()[0]
    stream = call(sc, cast, sc + ".Session", "RecordMonitor", GLib.Variant("(sa{sv})", ("Meta-0", {}))).unpack()[0]
    phase("sessions created")
    call(rd, session, rd + ".Session", "Start")
    phase("session started")
    time.sleep(0.3)
    # The seat gets its pointer and keyboard on first use; the clipboard
    # library only creates a data device once the seat has a keyboard, so
    # touch both before the app starts, as a real desktop would have.
    call(rd, session, rd + ".Session", "NotifyPointerMotionAbsolute", GLib.Variant("(sdd)", (stream, 10.0, 10.0)))
    for pressed in (True, False):
        call(rd, session, rd + ".Session", "NotifyKeyboardKeysym", GLib.Variant("(ub)", (0xffe1, pressed)))
        time.sleep(0.05)
    time.sleep(1.0)
    phase("seat ready")
    notes = tempfile.mkdtemp(prefix="probe-notes-")
    photo = os.path.join(notes, "..", "probe-photo.png")
    shutil.copy(sys.argv[2], photo)
    env = dict(os.environ, WAYLAND_DISPLAY=name, JJB_NOTES_DIR=notes, COSMIC_SINGLE_INSTANCE="false",
               XDG_DATA_HOME=tempfile.mkdtemp(prefix="probe-data-"),
               JJB_SCRIPT="wait:60000", RUST_LOG="attic=debug", WAYLAND_DEBUG="1")
    env.pop("DISPLAY", None)
    applog = open(f"{notes}/app.log", "wb")
    app = subprocess.Popen([sys.argv[1]], env=env, stdout=applog, stderr=subprocess.STDOUT)
    phase("app launched")
    time.sleep(7)
    genv = dict(os.environ, WAYLAND_DISPLAY=name, GDK_BACKEND="wayland", DBUS_SESSION_BUS_ADDRESS=addr)
    genv.pop("DISPLAY", None)
    srclog = open(f"{notes}/source.log", "wb")
    src = subprocess.Popen(["python3", os.path.join(S, "dnd-source.py"), photo], env=genv, stdout=srclog, stderr=subprocess.STDOUT)
    time.sleep(4)
    def move(x, y):
        call(rd, session, rd + ".Session", "NotifyPointerMotionAbsolute", GLib.Variant("(sdd)", (stream, float(x), float(y))))
    def button(pressed):
        call(rd, session, rd + ".Session", "NotifyPointerButton", GLib.Variant("(ib)", (272, pressed)))
    # Focus-click the source window first (its centre), then press and drag.
    phase("dragging")
    sx, sy = int(sys.argv[3]), int(sys.argv[4])
    tx, ty = int(sys.argv[5]), int(sys.argv[6])
    move(sx, sy); time.sleep(0.3); button(True); time.sleep(0.1); button(False); time.sleep(0.5)
    move(sx, sy); time.sleep(0.2); button(True); time.sleep(0.3)
    steps = 25
    for i in range(1, steps + 1):
        move(sx + (tx - sx) * i / steps, sy + (ty - sy) * i / steps); time.sleep(0.06)
    time.sleep(1.0)
    button(False)
    time.sleep(3)
    phase("collecting")
    print("=== source ==="); print(open(f"{notes}/source.log").read()[-1500:])
    print("=== app log (dnd) ===")
    import re
    for line in open(f"{notes}/app.log", errors="replace"):
        line = re.sub(r"\x1b\[[0-9;]*m", "", line).rstrip()
        if (re.search(r"wl_data_|drag over|Dropped|importing|offer|attic::", line, re.I)
                and not re.search(r"wl_data_device_manager|\.global\(|wl_registry", line)):
            print(line[:220])
    print("=== mutter log (dnd) ===")
    for line in open(mlog.name, errors="replace"):
        if re.search(r"dnd|drag|data.device|data_offer|data_source|Drop", line, re.I):
            print(line.rstrip()[:230])
    print("=== notes ==="); 
    for f in glob.glob(f"{notes}/*.md"):
        print(f, "::", [l for l in open(f).read().splitlines() if l.startswith("![")])
finally:
    for d in (app, src):
        if d is None: continue
        try: d.terminate()
        except Exception: pass
    for d in (mutter, bus):
        try:
            os.killpg(d.pid, 15); d.wait(timeout=2)
        except Exception:
            try: os.killpg(d.pid, 9)
            except Exception: pass
