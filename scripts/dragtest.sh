#!/usr/bin/env bash
# Real X pointer drag against the GUI under Xvfb. Usage: dragtest.sh <outdir> "x1 y1 x2 y2" ...
set -u
S=${SHOT_VENV_DIR:?dir holding venv/ with python-xlib and home/}; OUT=$1; shift; mkdir -p "$OUT"
D=":$((70 + RANDOM % 20))"
Xvfb "$D" -screen 0 1024x640x24 >/dev/null 2>&1 & XP=$!
sleep 1
env -u WAYLAND_DISPLAY DISPLAY="$D" HOME="$S/home" "$(cd "$(dirname "$0")/.." && pwd)"/target/debug/retraktor-gui >"$OUT/log.txt" 2>&1 & AP=$!
trap 'kill $AP $XP 2>/dev/null' EXIT
sleep "${WARM:-40}"
grab(){ ffmpeg -loglevel error -y -f x11grab -video_size 1024x640 -i "$D" -frames:v 1 "$OUT/$1.png"; }
grab 0-before
n=1
for d in "$@"; do
  DISPLAY="$D" "$S/venv/bin/python" - $d <<'P'
import sys,time
from Xlib import X,display
from Xlib.ext import xtest
x1,y1,x2,y2=map(int,sys.argv[1:5])
d=display.Display()
def mv(x,y): xtest.fake_input(d,X.MotionNotify,x=x,y=y); d.sync(); time.sleep(0.15)
mv(x1,y1); time.sleep(0.5)
xtest.fake_input(d,X.ButtonPress,1); d.sync(); time.sleep(0.15)
if (x1,y1)!=(x2,y2):
    for i in range(1,11): mv(x1+(x2-x1)*i//10, y1+(y2-y1)*i//10)
    time.sleep(0.3)
xtest.fake_input(d,X.ButtonRelease,1); d.sync(); time.sleep(1.0)
P
  grab $n-after; n=$((n+1))
done
