#!/usr/bin/env bash
# ReTraktor user launcher
set -e

CLI_BIN="$(command -v retraktor-cli 2>/dev/null || echo "/usr/local/bin/retraktor-cli")"
if [ ! -x "$CLI_BIN" ]; then
    CLI_BIN="/usr/bin/retraktor-cli"
fi

if [ "$#" -eq 0 ]; then
    exec "$CLI_BIN" run
else
    exec "$CLI_BIN" "$@"
fi
