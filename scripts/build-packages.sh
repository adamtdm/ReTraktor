#!/usr/bin/env bash
# ==============================================================================
# ReTraktor: Package Builder (.deb, AppImage, tar.gz)
# Builds ready-to-install packages for Linux
# ==============================================================================

set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

VERSION="$(grep -m1 '^version' Cargo.toml | awk -F '"' '{print $2}')"
ARCH="$(uname -m)"
if [ "$ARCH" = "x86_64" ]; then
    DEB_ARCH="amd64"
elif [ "$ARCH" = "aarch64" ]; then
    DEB_ARCH="arm64"
else
    DEB_ARCH="$ARCH"
fi

DIST_DIR="$SCRIPT_DIR/dist"
mkdir -p "$DIST_DIR"

echo "=== Building ReTraktor v$VERSION ($ARCH) ==="
cargo build --workspace --release

# 1. Build Standalone Portable Tarball
echo "=== Packaging Tarball ==="
TAR_STAGE="$(mktemp -d)"
mkdir -p "$TAR_STAGE/retraktor-$VERSION/bin"
mkdir -p "$TAR_STAGE/retraktor-$VERSION/assets"

cp target/release/retraktor-gui "$TAR_STAGE/retraktor-$VERSION/bin/"
cp target/release/retraktor-cli "$TAR_STAGE/retraktor-$VERSION/bin/"
cp assets/retraktor-wrapper.sh "$TAR_STAGE/retraktor-$VERSION/bin/retraktor"
cp -r assets/* "$TAR_STAGE/retraktor-$VERSION/assets/"
cp install.sh "$TAR_STAGE/retraktor-$VERSION/"
cp README.md "$TAR_STAGE/retraktor-$VERSION/"

tar -czf "$DIST_DIR/retraktor-${VERSION}-${ARCH}-linux.tar.gz" -C "$TAR_STAGE" "retraktor-$VERSION"
rm -rf "$TAR_STAGE"
echo "Created: $DIST_DIR/retraktor-${VERSION}-${ARCH}-linux.tar.gz"

# 2. Build Debian package (.deb)
if command -v dpkg-deb >/dev/null 2>&1; then
    echo "=== Packaging .deb via dpkg-deb ==="
    DEB_STAGE="$(mktemp -d)"
    DEB_ROOT="$DEB_STAGE/retraktor_${VERSION}_${DEB_ARCH}"

    mkdir -p "$DEB_ROOT/DEBIAN"
    mkdir -p "$DEB_ROOT/usr/bin"
    mkdir -p "$DEB_ROOT/lib/udev/rules.d"
    mkdir -p "$DEB_ROOT/etc/security/limits.d"
    mkdir -p "$DEB_ROOT/usr/share/applications"
    mkdir -p "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps"

    cp target/release/retraktor-gui "$DEB_ROOT/usr/bin/retraktor-gui"
    cp target/release/retraktor-cli "$DEB_ROOT/usr/bin/retraktor-cli"
    cp assets/retraktor-wrapper.sh "$DEB_ROOT/usr/bin/retraktor"
    chmod 0755 "$DEB_ROOT/usr/bin/retraktor-gui" "$DEB_ROOT/usr/bin/retraktor-cli" "$DEB_ROOT/usr/bin/retraktor"

    cp assets/99-retraktor-ni.rules "$DEB_ROOT/lib/udev/rules.d/"
    cp assets/99-retraktor-realtime.conf "$DEB_ROOT/etc/security/limits.d/"
    cp assets/retraktor.desktop "$DEB_ROOT/usr/share/applications/"
    cp assets/retraktor.svg "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps/"

    cat > "$DEB_ROOT/DEBIAN/control" << EOF
Package: retraktor
Version: ${VERSION}
Section: sound
Priority: optional
Architecture: ${DEB_ARCH}
Depends: libasound2, libusb-1.0-0, libhidapi-hidraw0 | libhidapi-libusb0
Recommends: pipewire, wireplumber
Maintainer: Adam Meek <adamtdm@users.noreply.github.com>
Description: Clean-room Linux re-implementation of Traktor Pro 4
 ReTraktor provides complete 4-deck DJ mixing, reactive CSI 1.0 QML scripting,
 .nml/.tsi library interoperability, and Traktor Kontrol & MX2 HID/USB hardware support.
EOF

    cp debian/postinst "$DEB_ROOT/DEBIAN/postinst"
    cp debian/postrm "$DEB_ROOT/DEBIAN/postrm"
    chmod 0755 "$DEB_ROOT/DEBIAN/postinst" "$DEB_ROOT/DEBIAN/postrm"

    dpkg-deb --build "$DEB_ROOT" "$DIST_DIR/retraktor_${VERSION}_${DEB_ARCH}.deb"
    rm -rf "$DEB_STAGE"
    echo "Created: $DIST_DIR/retraktor_${VERSION}_${DEB_ARCH}.deb"
elif command -v cargo-deb >/dev/null 2>&1; then
    echo "=== Packaging .deb via cargo-deb ==="
    cargo deb -p retraktor-cli -o "$DIST_DIR/retraktor_${VERSION}_${DEB_ARCH}.deb"
    echo "Created: $DIST_DIR/retraktor_${VERSION}_${DEB_ARCH}.deb"
else
    echo "Note: dpkg-deb / cargo-deb not found. Skipping .deb creation on this host."
fi

# 3. Build AppImage (if appimagetool is available or prepare AppDir)
echo "=== Packaging AppDir ==="
APPDIR="$DIST_DIR/ReTraktor.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"

cp target/release/retraktor-gui "$APPDIR/usr/bin/retraktor-gui"
cp target/release/retraktor-cli "$APPDIR/usr/bin/retraktor-cli"
cp assets/retraktor-wrapper.sh "$APPDIR/usr/bin/retraktor"
chmod 0755 "$APPDIR/usr/bin/retraktor-gui" "$APPDIR/usr/bin/retraktor-cli" "$APPDIR/usr/bin/retraktor"
cp assets/retraktor.desktop "$APPDIR/retraktor.desktop"
cp assets/retraktor.desktop "$APPDIR/usr/share/applications/"
cp assets/retraktor.svg "$APPDIR/retraktor.svg"
cp assets/retraktor.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/"

cat > "$APPDIR/AppRun" << 'EOF'
#!/bin/sh
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/retraktor" "$@"
EOF
chmod 0755 "$APPDIR/AppRun"

if command -v appimagetool >/dev/null 2>&1; then
    appimagetool "$APPDIR" "$DIST_DIR/ReTraktor-${ARCH}.AppImage"
    echo "Created: $DIST_DIR/ReTraktor-${ARCH}.AppImage"
else
    echo "AppDir prepared at $APPDIR (Run appimagetool on Linux to bundle into ReTraktor.AppImage)"
fi

echo "=== All Available Packages in $DIST_DIR ==="
ls -lh "$DIST_DIR"
