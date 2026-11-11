#!/usr/bin/env bash
# Records the GUI headlessly to an MP4 for reviewing motion (needs xvfb and ffmpeg).
# Usage: scripts/record.sh <out.mp4> [steps] [seconds]   Env: SHOT_HOME, SHOT_SIZE (default 1280x800), REC_AFTER (step file to wait for).
# Recording starts once the last step's screenshot exists, so the steps set the scene: e.g. "loadA,loadB,wait,playA,playB,Mix".
set -euo pipefail
OUT="${1:?out.mp4}"; STEPS="${2:-loadA,loadB,wait,playA,playB,Mix}"; SECS="${3:-5}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"; SIZE="${SHOT_SIZE:-1280x800}"
TMP="$(mktemp -d)"; HOME_DIR="${SHOT_HOME:-$TMP/home}"; mkdir -p "$HOME_DIR"
cargo build --manifest-path "$REPO/Cargo.toml" -p retraktor-gui -q
D=":$((90 + RANDOM % 9))"
Xvfb "$D" -screen 0 "${SIZE}x24" >/dev/null 2>&1 & XPID=$!
sleep 1
# Trailing waits keep the app alive while ffmpeg records.
env -u WAYLAND_DISPLAY DISPLAY="$D" HOME="$HOME_DIR" RETRAKTOR_SHOT_DIR="$TMP/shots" SHOT_STEPS="$STEPS,wait,wait,wait,wait,wait,wait" \
  "$REPO/target/debug/retraktor-gui" >"$TMP/log.txt" 2>&1 & APID=$!
trap 'kill $APID $XPID 2>/dev/null || true' EXIT
N=$(awk -F, '{print NF}' <<<"$STEPS")
for _ in $(seq 600); do [ "$(ls "$TMP/shots" 2>/dev/null | grep -c png)" -ge "$N" ] && break; sleep 0.5; done
ffmpeg -loglevel error -y -f x11grab -framerate 30 -video_size "$SIZE" -i "$D" -t "$SECS" -pix_fmt yuv420p "$OUT"
echo "$OUT"
