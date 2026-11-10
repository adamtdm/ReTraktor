#!/usr/bin/env bash
# Headless screenshots of the GUI for design review (needs xvfb: sudo apt install xvfb).
# Usage: scripts/screenshot.sh <out-dir> [steps]   e.g. scripts/screenshot.sh /tmp/shots "Mix,prefs:Library,nav:Missing"
# Env: SHOT_HOME (HOME for the app; default: a fresh empty dir), SHOT_SIZE (default 1280x800), SHOT_WARM (frames, default 300).
set -euo pipefail
OUT="${1:?out dir}"; STEPS="${2:-Mix,FourDeck,Browser,prefs}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"
HOME_DIR="${SHOT_HOME:-$(mktemp -d)}"
cargo build --manifest-path "$REPO/Cargo.toml" -p retraktor-gui -q
env -u WAYLAND_DISPLAY HOME="$HOME_DIR" RETRAKTOR_SHOT_DIR="$OUT" SHOT_STEPS="$STEPS" \
  timeout 900 xvfb-run -a -s "-screen 0 ${SHOT_SIZE:-1280x800}x24" "$REPO/target/debug/retraktor-gui" >"$OUT/log.txt" 2>&1 || true
ls "$OUT"
