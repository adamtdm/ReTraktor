#!/usr/bin/env bash
# Installs the latest ReTraktor test build from the dev machine's local web server.
# Published to ~/pages/retraktor/install.sh by scripts/deploy.sh; edit it here, not there.
set -euo pipefail
BASE="${RETRAKTOR_URL:-http://retraktor.local}"
[ "$(uname -m)" = "x86_64" ] || { echo "This build is x86_64 only (you have $(uname -m))." >&2; exit 1; }

# Older source installs put copies in /usr/local/bin and ~/.cargo/bin, which shadow /usr/bin.
for f in /usr/local/bin/retraktor /usr/local/bin/retraktor-gui /usr/local/bin/retraktor-cli; do
    [ -e "$f" ] && { echo "Removing stale $f"; sudo rm -f "$f"; }
done
for f in "$HOME/.cargo/bin/retraktor-gui" "$HOME/.cargo/bin/retraktor-cli"; do
    [ -e "$f" ] && { echo "Removing stale $f"; rm -f "$f"; }
done
pkill -x retraktor-gui 2>/dev/null && echo "Closed running ReTraktor" || true

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
echo "Downloading ReTraktor from $BASE ..."
curl -fSL "$BASE/retraktor_latest_amd64.deb" -o "$TMP/retraktor.deb"
sudo apt-get install -y --reinstall --allow-downgrades "$TMP/retraktor.deb"
hash -r
echo
echo "Installed $(dpkg-query -W -f='${Version}' retraktor) -> $(command -v retraktor-gui)"
echo "Start it from the app menu or run: retraktor"
