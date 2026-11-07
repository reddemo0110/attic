#!/usr/bin/env python3
"""A GTK 4 window that offers one file to drag, the way a file manager does
(copy / move / link / ask), deleting it after a move. Used by dnd-probe.py."""
import sys, gi, time
T0=time.time()
def P(*a): print(f"{time.time()-T0:6.2f}s", *a, flush=True)
gi.require_version('Gtk', '4.0'); gi.require_version('Gdk', '4.0')
from gi.repository import Gtk, Gdk, Gio, GLib
path = sys.argv[1]
app = Gtk.Application(application_id="dev.attic.dndsource")
def on_activate(app):
    win = Gtk.ApplicationWindow(application=app, title="dnd source", default_width=300, default_height=200)
    label = Gtk.Label(label="drag me")
    win.set_child(label)
    src = Gtk.DragSource(actions=Gdk.DragAction.COPY | Gdk.DragAction.MOVE | Gdk.DragAction.LINK | Gdk.DragAction.ASK)
    f = Gio.File.new_for_path(path)
    src.set_content(Gdk.ContentProvider.new_for_value(Gdk.FileList.new_from_list([f])))
    src.connect("drag-begin", lambda s, d: P("source: drag-begin"))
    def ended(s, d, dm):
        P("source: drag-end delete=", dm)
        if dm:
            # What a file manager does after a move: the original goes.
            import os; os.remove(path); P("source: removed", path)
    src.connect("drag-end", ended)
    src.connect("drag-cancel", lambda s, d, r: P("source: drag-cancel", r) or False)
    win.add_controller(src)
    motion = Gtk.EventControllerMotion()
    motion.connect("enter", lambda c, x, y: P(f"source: pointer enter {x:.0f},{y:.0f}"))
    win.add_controller(motion)
    win.present()
app.connect("activate", on_activate)
app.run([])
