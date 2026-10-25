#!/usr/bin/env bash
# ==============================================================================
# ReTraktor: Native Desktop & CLI Launcher
# Dispatches to the native graphical desktop interface (retraktor-gui) by default,
# or to retraktor-cli when CLI subcommands or headless environments are detected.
# ==============================================================================
set -e

# Record launch attempt in /tmp/retraktor.log
echo "[$(date '+%Y-%m-%d %H:%M:%S')] retraktor launcher called with args: '$*'" >> /tmp/retraktor.log

GUI_BIN="$(command -v retraktor-gui 2>/dev/null || echo "")"
if [ -z "$GUI_BIN" ] || [ ! -x "$GUI_BIN" ]; then
    if [ -x "/usr/local/bin/retraktor-gui" ]; then
        GUI_BIN="/usr/local/bin/retraktor-gui"
    elif [ -x "/usr/bin/retraktor-gui" ]; then
        GUI_BIN="/usr/bin/retraktor-gui"
    fi
fi

CLI_BIN="$(command -v retraktor-cli 2>/dev/null || echo "")"
if [ -z "$CLI_BIN" ] || [ ! -x "$CLI_BIN" ]; then
    if [ -x "/usr/local/bin/retraktor-cli" ]; then
        CLI_BIN="/usr/local/bin/retraktor-cli"
    elif [ -x "/usr/bin/retraktor-cli" ]; then
        CLI_BIN="/usr/bin/retraktor-cli"
    fi
fi

# 1. Handle CLI subcommands directly
if [ "$#" -gt 0 ]; then
    case "$1" in
        inspect-nml|inspect-tsi|inspect-stripe|inspect-transients|inspect-csi|dump-prop-bus|list-devices|udev-rules|doctor|--doctor|diagnose|simulate-mix|tui|--help|-h|help)
            if [ -n "$CLI_BIN" ] && [ -x "$CLI_BIN" ]; then
                exec "$CLI_BIN" "$@"
            fi
            ;;
        gui)
            shift
            if [ -n "$GUI_BIN" ] && [ -x "$GUI_BIN" ]; then
                exec "$GUI_BIN" "$@"
            fi
            ;;
    esac
fi

# 2. Launch native desktop GUI window if display server (Wayland or X11) is active
if [ -n "${WAYLAND_DISPLAY:-}" ] || [ -n "${DISPLAY:-}" ]; then
    if [ -n "$GUI_BIN" ] && [ -x "$GUI_BIN" ]; then
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] Launching native ReTraktor Pro 4 GUI window..." >> /tmp/retraktor.log
        exec "$GUI_BIN" "$@"
    fi
fi

# 3. Fallback to CLI or TUI if no graphical display is attached (e.g. headless SSH session)
if [ -n "$CLI_BIN" ] && [ -x "$CLI_BIN" ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Graphical display unavailable; falling back to terminal player." >> /tmp/retraktor.log
    exec "$CLI_BIN" "$@"
fi

echo "ERROR: Neither retraktor-gui nor retraktor-cli could be located." | tee -a /tmp/retraktor.log
exit 1
