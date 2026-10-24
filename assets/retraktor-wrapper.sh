#!/usr/bin/env bash
# ReTraktor user launcher
set -e

if [ "$#" -eq 0 ]; then
    exec /usr/bin/retraktor-cli simulate-mix
else
    exec /usr/bin/retraktor-cli "$@"
fi
