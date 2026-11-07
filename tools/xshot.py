#!/usr/bin/env python3
"""Headless-ish visual check for Attic.

Runs the app on Xwayland (DISPLAY, WAYLAND_DISPLAY unset), optionally drives
it through the app's JJB_SCRIPT hook, and captures the window with XGetImage.

    tools/xshot.py out.png                                   # launch, wait, capture
    tools/xshot.py out.png --script 'new;type:Hello #demo;wait:1500' --wait 6
    tools/xshot.py out.png --nested 1600x1000 --click 31,24  # own mutter, open File

`--nested WxH` starts a headless mutter (on a private session bus) with its own
Xwayland and runs the app there: nothing reaches the real desktop, so
`--click X,Y` (window pixels, repeatable) is safe to use for menus and popovers.
Clicks go through mutter's own virtual pointer (`org.gnome.Mutter.RemoteDesktop`
on that bus; headless mutter ignores XTEST from Xwayland). Popups the app opens
in their own X window are composited into the capture. Without `--nested`,
`--click` is refused: on the session's rootless Xwayland an injected click would
land on whatever is on the real screen.

Needs python-xlib and Pillow. iced's own window::screenshot drops editor text
and menu labels, and XTEST/portal input injection hangs unattended on COSMIC,
which is why this exists. See src/debug_script.rs for the step language.
"""
import argparse, os, subprocess, sys, time
from Xlib import X, XK, display
from PIL import Image

class _Timeout(Exception):
    pass

def _find_window(w, depth=0):
    """Recursive search for the app's top-level window (depth-limited: Xwayland
    puts it two levels below root)."""
    if depth > 3:
        return None, None
    try:
        attrs = w.get_attributes()
        geom = w.get_geometry()
        cls = w.get_wm_class()
    except Exception:
        return None, None
    if attrs.map_state == X.IsViewable and geom.width > 100 and cls and any("attic" in c.lower() for c in cls):
        return w, geom
    try:
        children = w.query_tree().children
    except Exception:
        return None, None
    for c in children:
        found, g = _find_window(c, depth + 1)
        if found is not None:
            return found, g
    return None, None

def find_window(display_name, tries=6, timeout=3):
    """Walk the X tree with a hard timeout per attempt; python-xlib occasionally
    blocks on a window that is mid-creation, so reconnect and retry."""
    import signal
    def on_alarm(signum, frame):
        raise _Timeout()
    old = signal.signal(signal.SIGALRM, on_alarm)
    try:
        for attempt in range(tries):
            d = display.Display(display_name)
            signal.alarm(timeout)
            try:
                win, geom = _find_window(d.screen().root)
                signal.alarm(0)
                if win is not None:
                    return d, win, geom
            except _Timeout:
                print(f"x walk timed out (attempt {attempt + 1}), retrying", file=sys.stderr, flush=True)
            finally:
                signal.alarm(0)
            try:
                d.close()
            except Exception:
                pass
            time.sleep(1)
    finally:
        signal.signal(signal.SIGALRM, old)
    return None, None, None

def log(*a):
    if os.environ.get("XSHOT_DEBUG"):
        print(*a, file=sys.stderr, flush=True)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--binary", default="target/debug/attic")
    ap.add_argument("--display", default=os.environ.get("DISPLAY", ":1"))
    ap.add_argument("--wait", type=float, default=4.0, help="seconds after launch before acting")
    ap.add_argument("--script", default=None, help="JJB_SCRIPT steps, e.g. 'new;type:Hi;wait:1500'")
    ap.add_argument("--settle", type=float, default=1.5, help="seconds before capture")
    ap.add_argument("--keep", action="store_true", help="leave the app running")
    ap.add_argument("--nested", default=None, metavar="WxH",
                    help="run inside a headless mutter of this size with its own Xwayland")
    ap.add_argument("--click", action="append", default=[], metavar="X,Y",
                    help="left-click here (window pixels) before capture; repeatable, in order")
    ap.add_argument("--key", action="append", default=[], metavar="KEYSYM",
                    help="press and release this X keysym (decimal or 0x hex) after the clicks; repeatable")
    ap.add_argument("--notes-dir", default=None,
                    help="notes directory for the run (default: a fresh scratch dir, so the "
                         "real notes are never touched; pass the real path to see them)")
    args = ap.parse_args()

    if (args.click or args.key) and not args.nested:
        print("--click/--key need --nested: on the session's Xwayland they would hit the real desktop",
              file=sys.stderr)
        return 2
    if args.key:
        args.click = args.click or []
    mutter = None
    bus = None
    bus_addr = None
    if args.nested:
        import glob, re, tempfile
        # A run cut short (a timeout, Ctrl-C) can leave its mutter behind,
        # spinning; they are all ours by name, so sweep before starting.
        # (The bracket keeps the pattern from matching a shell that quotes it.)
        subprocess.run(["pkill", "-9", "-f", "mutter --headles[s] .*--wayland-display xshot-"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        # A private session bus for mutter: its RemoteDesktop service is how
        # we move the pointer, and nothing of ours lands on the real one.
        bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--print-address=1"],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               start_new_session=True)
        bus_addr = bus.stdout.readline().decode().strip()
        mlog = tempfile.NamedTemporaryFile(prefix="xshot-mutter-", suffix=".log", delete=False)
        name = f"xshot-{os.getpid()}"
        # Its Xwayland takes a cookie from a fresh .mutter-Xwaylandauth.*
        # in the runtime dir; whichever appears after launch is ours.
        runtime = os.environ.get("XDG_RUNTIME_DIR", "/tmp")
        auth_pat = os.path.join(runtime, ".mutter-Xwaylandauth.*")
        auth_before = set(glob.glob(auth_pat))
        mutter = subprocess.Popen(
            ["mutter", "--headless", "--wayland", "--virtual-monitor", args.nested,
             "--wayland-display", name],
            env=dict(os.environ, DBUS_SESSION_BUS_ADDRESS=bus_addr),
            stdout=mlog, stderr=subprocess.STDOUT, start_new_session=True)
        for _ in range(100):
            time.sleep(0.1)
            m = re.search(r"Using public X11 display (:\d+)", open(mlog.name).read())
            if m:
                args.display = m.group(1)
                break
        else:
            print("nested mutter did not come up, see", mlog.name, file=sys.stderr)
            os.killpg(mutter.pid, 15)
            os.killpg(bus.pid, 15)
            return 1
        for _ in range(50):
            new_auth = sorted(set(glob.glob(auth_pat)) - auth_before, key=os.path.getmtime)
            if new_auth:
                os.environ["XAUTHORITY"] = new_auth[-1]
                break
            time.sleep(0.1)
        log("nested mutter on", args.display, "auth", os.environ.get("XAUTHORITY"))
    env = dict(os.environ, DISPLAY=args.display)
    env.pop("WAYLAND_DISPLAY", None)
    if args.notes_dir is None:
        import tempfile
        args.notes_dir = tempfile.mkdtemp(prefix="jjb-notes-")
        log("scratch notes dir:", args.notes_dir)
    env["JJB_NOTES_DIR"] = args.notes_dir
    # index.db lives under XDG_DATA_HOME and is shared by every instance:
    # a run with its own notes dir must not reindex the real app's index
    # out from under it (and vice versa), so give it its own.
    import tempfile as _tempfile
    env["XDG_DATA_HOME"] = _tempfile.mkdtemp(prefix="jjb-data-")
    env.setdefault("RUST_LOG", "attic=info,warn")
    # The app is single-instance: without this a real Attic already
    # running would be handed our launch and the scratch run would never
    # open a window.
    env["COSMIC_SINGLE_INSTANCE"] = "false"
    # XSHOT_LOG=path keeps the app's log (tracing writes to stdout).
    env["XSHOT_LOG"] = env.get("XSHOT_LOG", "")
    if args.script:
        env["JJB_SCRIPT"] = args.script
    log_file = open(env["XSHOT_LOG"], "wb") if env["XSHOT_LOG"] else subprocess.DEVNULL
    proc = subprocess.Popen([args.binary], env=env, stdout=log_file, stderr=subprocess.STDOUT)
    # A key left held on the X server (e.g. by a broken injection tool) would
    # auto-repeat into our window; switch X auto-repeat off while we capture.
    kb = None
    try:
        kb = display.Display(args.display)
        kb.change_keyboard_control(auto_repeat_mode=X.AutoRepeatModeOff)
        kb.sync()
    except Exception as e:
        log("could not disable autorepeat:", e)
    try:
        time.sleep(args.wait)
        log("launched, searching")
        d, win, geom = find_window(args.display)
        log("search done:", win is not None)
        if win is None:
            print("no Attic window found", file=sys.stderr)
            return 1
        log("settling")
        time.sleep(args.settle)
        geom = win.get_geometry()
        root = d.screen().root
        if args.click or args.key:
            from gi.repository import Gio, GLib
            conn = Gio.DBusConnection.new_for_address_sync(
                bus_addr,
                Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
                | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
                None, None)
            rd = "org.gnome.Mutter.RemoteDesktop"
            def call(path, iface, method, params=None):
                return conn.call_sync(rd, path, iface, method, params, None,
                                      Gio.DBusCallFlags.NONE, 5000, None)
            session = call("/org/gnome/Mutter/RemoteDesktop", rd, "CreateSession").unpack()[0]
            sess_iface = rd + ".Session"
            session_id = conn.call_sync(rd, session, "org.freedesktop.DBus.Properties", "Get",
                                        GLib.Variant("(ss)", (sess_iface, "SessionId")), None,
                                        Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
            # Absolute pointer motion is given against a screen-cast stream
            # of the monitor (the way gnome-remote-desktop does it); the
            # stream is never consumed, it only anchors the coordinates.
            sc = "org.gnome.Mutter.ScreenCast"
            cast = conn.call_sync(sc, "/org/gnome/Mutter/ScreenCast", sc, "CreateSession",
                                  GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", session_id)},)),
                                  None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
            stream = conn.call_sync(sc, cast, sc + ".Session", "RecordMonitor",
                                    GLib.Variant("(sa{sv})", ("Meta-0", {})),
                                    None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
            # Starting the remote-desktop session starts its screen cast too.
            call(session, sess_iface, "Start")
            time.sleep(0.3)
            origin = win.translate_coords(root, 0, 0)
            ox, oy = -origin.x, -origin.y
            for spec in args.click:
                x, y = (int(v) for v in spec.split(","))
                call(session, sess_iface, "NotifyPointerMotionAbsolute",
                     GLib.Variant("(sdd)", (stream, float(ox + x), float(oy + y))))
                time.sleep(0.2)
                qp = root.query_pointer()
                log("pointer now at", qp.root_x, qp.root_y, "wanted", ox + x, oy + y)
                for pressed in (True, False):
                    call(session, sess_iface, "NotifyPointerButton",
                         GLib.Variant("(ib)", (272, pressed)))  # BTN_LEFT
                    time.sleep(0.1)
                time.sleep(0.8)
            for spec in args.key:
                keysym = int(spec, 0)
                for pressed in (True, False):
                    call(session, sess_iface, "NotifyKeyboardKeysym",
                         GLib.Variant("(ub)", (keysym, pressed)))
                    time.sleep(0.1)
                time.sleep(0.5)
        raw = win.get_image(0, 0, geom.width, geom.height, X.ZPixmap, 0xFFFFFFFF)
        img = Image.frombytes("RGB", (geom.width, geom.height), raw.data, "raw", "BGRX")
        if args.click:
            # A menu or popover is its own override-redirect window of the
            # same X client (same resource-id base); paste those on top.
            origin = win.translate_coords(root, 0, 0)
            ox, oy = -origin.x, -origin.y
            for child in root.query_tree().children:
                try:
                    if child.id == win.id or (child.id ^ win.id) & 0xFFE00000:
                        continue
                    at = child.get_attributes()
                    if at.map_state != X.IsViewable or not at.override_redirect:
                        continue
                    cg = child.get_geometry()
                    pos = child.translate_coords(root, 0, 0)
                    cx, cy = -pos.x - ox, -pos.y - oy
                    log("popup", hex(child.id), cg.width, cg.height, "at", cx, cy)
                    craw = child.get_image(0, 0, cg.width, cg.height, X.ZPixmap, 0xFFFFFFFF)
                    img.paste(Image.frombytes("RGB", (cg.width, cg.height), craw.data, "raw", "BGRX"), (cx, cy))
                except Exception as e:
                    log("skip child:", e)
        img.save(args.out)
        print("saved", args.out, f"{geom.width}x{geom.height}")
        return 0
    finally:
        if kb is not None:
            try:
                kb.change_keyboard_control(auto_repeat_mode=X.AutoRepeatModeDefault)
                kb.sync()
            except Exception:
                pass
        if not args.keep:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
        # mutter does not go on SIGTERM once its Xwayland is gone (it spins
        # on a core instead), so give it a moment and then make sure.
        for daemon in (mutter, bus):
            if daemon is None:
                continue
            try:
                os.killpg(daemon.pid, 15)
                daemon.wait(timeout=2)
            except Exception:
                try:
                    os.killpg(daemon.pid, 9)
                except Exception:
                    pass

if __name__ == "__main__":
    sys.exit(main())
