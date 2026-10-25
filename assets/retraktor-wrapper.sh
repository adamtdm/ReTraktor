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

# If launched without an interactive TTY (e.g. from desktop launcher / app menu), spawn a terminal window
if [ ! -t 0 ] || [ ! -t 1 ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Spawning terminal emulator..." >> /tmp/retraktor.log
    for TERM_EMU in xdg-terminal-exec cosmic-term gnome-terminal x-terminal-emulator alacritty kitty xterm; do
        if command -v "$TERM_EMU" >/dev/null 2>&1; then
            case "$TERM_EMU" in
                gnome-terminal)
                    exec gnome-terminal --title="ReTraktor DJ" -- "$CLI_BIN" "$@"
                    ;;
                cosmic-term)
                    exec cosmic-term -e "$CLI_BIN" "$@"
                    ;;
                *)
                    exec "$TERM_EMU" -e "$CLI_BIN" "$@"
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
