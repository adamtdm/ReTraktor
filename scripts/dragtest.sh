#!/usr/bin/env bash
# Real X pointer and keyboard input against the GUI under Xvfb, with a screenshot after every step.
# Usage: dragtest.sh <outdir> STEP ...
#   "x1 y1 x2 y2"  press at x1,y1, drag to x2,y2, release (equal points = click)
#   "dclick x y"   double-click        "type some text"   type text
#   "key Return"   press a key (X keysym name)            "wait 2"   sleep seconds
# Env: SHOT_VENV_DIR holds venv/ (with python-xlib) and home/ (HOME for the app); WARM seconds before step 1.
set -u
S=${SHOT_VENV_DIR:?dir holding venv/ with python-xlib and home/}; OUT=$1; shift; mkdir -p "$OUT"
D=":$((70 + RANDOM % 20))"
Xvfb "$D" -screen 0 1024x640x24 >/dev/null 2>&1 & XP=$!
sleep 1
env -u WAYLAND_DISPLAY DISPLAY="$D" HOME="$S/home" "$(cd "$(dirname "$0")/.." && pwd)"/target/debug/retraktor-gui >"$OUT/log.txt" 2>&1 & AP=$!
trap 'kill $AP $XP 2>/dev/null' EXIT
sleep "${WARM:-40}"
DISPLAY="$D" OUT="$OUT" "$S/venv/bin/python" - "$@" <<'P'
import os, subprocess, sys, time
from Xlib import X, XK, display
from Xlib.ext import xtest
d = display.Display()
def grab(name):
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size", "1024x640",
                    "-i", os.environ["DISPLAY"], "-frames:v", "1", f"{os.environ['OUT']}/{name}.png"], check=True)
def mv(x, y): xtest.fake_input(d, X.MotionNotify, x=x, y=y); d.sync(); time.sleep(0.12)
def btn(kind): xtest.fake_input(d, kind, 1); d.sync(); time.sleep(0.06)
def key(sym, shift=False):
    code = d.keysym_to_keycode(XK.string_to_keysym(sym))
    if shift: xtest.fake_input(d, X.KeyPress, d.keysym_to_keycode(XK.XK_Shift_L))
    xtest.fake_input(d, X.KeyPress, code); xtest.fake_input(d, X.KeyRelease, code)
    if shift: xtest.fake_input(d, X.KeyRelease, d.keysym_to_keycode(XK.XK_Shift_L))
    d.sync(); time.sleep(0.08)
# No window manager under Xvfb: hand the app window keyboard focus ourselves, or text fields never take input.
app = next(w for w in d.screen().root.query_tree().children if w.get_attributes().map_state == X.IsViewable)
d.set_input_focus(app, X.RevertToParent, X.CurrentTime); d.sync(); mv(512, 320)
grab("0-before")
for n, step in enumerate(sys.argv[1:], 1):
    verb, *rest = step.split(" ", 1)
    arg = rest[0] if rest else ""
    if verb == "dclick":
        x, y = map(int, arg.split()); mv(x, y); time.sleep(0.3)
        for _ in range(2): btn(X.ButtonPress); btn(X.ButtonRelease)
    elif verb == "type":
        for ch in arg:
            sym = {" ": "space", "-": "minus", ".": "period"}.get(ch, ch)
            key(sym, ch.isupper())
    elif verb == "key": key(arg)
    elif verb == "wait": time.sleep(float(arg))
    else:
        x1, y1, x2, y2 = map(int, step.split()); mv(x1, y1); time.sleep(0.4); btn(X.ButtonPress)
        if (x1, y1) != (x2, y2):
            for i in range(1, 11): mv(x1 + (x2 - x1) * i // 10, y1 + (y2 - y1) * i // 10)
            time.sleep(0.3)
        btn(X.ButtonRelease)
    time.sleep(1.0)
    grab(f"{n}-{verb if verb in ('dclick', 'type', 'key', 'wait') else 'drag'}")
P
