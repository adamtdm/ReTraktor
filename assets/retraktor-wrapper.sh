#!/usr/bin/env bash
# ReTraktor user launcher
set -e

# Record launch attempt in /tmp/retraktor.log
echo "[$(date '+%Y-%m-%d %H:%M:%S')] retraktor launcher called with args: '$*'" >> /tmp/retraktor.log

CLI_BIN="$(command -v retraktor-cli 2>/dev/null || echo "/usr/local/bin/retraktor-cli")"
if [ ! -x "$CLI_BIN" ]; then
    CLI_BIN="/usr/bin/retraktor-cli"
fi

if [ ! -x "$CLI_BIN" ]; then
    echo "ERROR: retraktor-cli binary not found in /usr/local/bin or /usr/bin" | tee -a /tmp/retraktor.log
    exit 1
fi

# If already inside a terminal spawned by this wrapper, bypass
if [ "${1:-}" = "--in-terminal" ]; then
    shift
    exec "$CLI_BIN" "$@"
fi

# If launched from GUI / desktop launcher without an interactive TTY, spawn a real terminal window
if [ ! -t 0 ] || [ ! -t 1 ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Spawning terminal emulator..." >> /tmp/retraktor.log
    for TERM_EMU in cosmic-term gnome-terminal x-terminal-emulator alacritty kitty xterm; do
        if command -v "$TERM_EMU" >/dev/null 2>&1; then
            case "$TERM_EMU" in
                gnome-terminal)
                    exec gnome-terminal --title="ReTraktor DJ" -- "$0" --in-terminal run "$@"
                    ;;
                cosmic-term)
                    exec cosmic-term -e "$0" --in-terminal run "$@"
                    ;;
                *)
                    exec "$TERM_EMU" -e "$0" --in-terminal run "$@"
                    ;;
            esac
        fi
    done
fi

if [ "$#" -eq 0 ]; then
    exec "$CLI_BIN" run
else
    exec "$CLI_BIN" "$@"
fi
