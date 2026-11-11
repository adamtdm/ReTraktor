#!/usr/bin/env bash
# Deploy a test build: run the tests, push the current branch, build the .deb and publish it
# to the local page (http://retraktor.local, also reachable over Tailscale).
# Usage: scripts/deploy.sh ["what to test in this build" ...]   (default: the last commit subject)
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

PAGE="${RETRAKTOR_PAGE:-$HOME/pages/retraktor}"
LAN="http://192.168.68.105/pages/retraktor"
TAILNET="https://pop-os-1.tailafd5c7.ts.net/pages/retraktor"

[ -z "$(git status --porcelain)" ] || { echo "Uncommitted changes: commit first, so the build matches git." >&2; exit 1; }
[ -f "$PAGE/install.sh" ] || { echo "No installer at $PAGE/install.sh" >&2; exit 1; }

cargo test --workspace -q
git push origin HEAD

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)+$(date +%Y%m%d.%H%M)"
RETRAKTOR_VERSION="$VERSION" scripts/build-packages.sh
install -m 644 "dist/retraktor_${VERSION}_amd64.deb" "$PAGE/retraktor_latest_amd64.deb"

esc() { sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g'; }
[ $# -gt 0 ] || set -- "$(git log -1 --format=%s)"
NOTES="$(printf '%s\n' "$@" | esc | sed 's|.*|<li>&</li>|' | tr -d '\n')"
cat > "$PAGE/index.html" <<EOF
<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>ReTraktor test builds</title>
<style>:root{color-scheme:dark;--bg:#16181c;--fg:#e6e8eb;--mute:#9aa0a8;--acc:#5aa9e6}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif}
main{max-width:760px;margin:0 auto;padding:32px 16px}h1{margin:0 0 4px}p.m{color:var(--mute);margin:0 0 24px}
pre{background:#0e0f12;border:1px solid #2a2e35;border-radius:6px;padding:14px;overflow-x:auto;font-size:14px}
code{color:var(--acc)}a{color:var(--acc)}li{margin:4px 0}</style></head><body><main>
<h1>ReTraktor</h1><p class="m">Test build $VERSION · $(git log -1 --format='%h %s' | esc) · $(date '+%-d %b %Y %H:%M')</p>
<h2>Install or update</h2><pre><code>curl -fsSL http://retraktor.local/install.sh | bash</code></pre>
<p>If <code>retraktor.local</code> doesn't resolve on that machine, use the IP instead:</p>
<pre><code>curl -fsSL $LAN/install.sh | RETRAKTOR_URL=$LAN bash</code></pre>
<p>Away from home, over Tailscale:</p>
<pre><code>curl -fsSL $TAILNET/install.sh | RETRAKTOR_URL=$TAILNET bash</code></pre>
<p>Or download the package directly: <a href="retraktor_latest_amd64.deb">retraktor_latest_amd64.deb</a> (x86_64).</p>
<h2>What to test in this build</h2><ul>$NOTES</ul>
</main></body></html>
EOF
chmod -R a+rX "$PAGE"
echo "Deployed $VERSION to $PAGE"
