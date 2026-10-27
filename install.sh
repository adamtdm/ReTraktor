#!/usr/bin/env bash
# ==============================================================================
# ReTraktor: 1-Command Universal Linux Installer
# https://github.com/adamtdm/ReTraktor
#
# Installs ReTraktor, audio libraries, Traktor Kontrol & MX2 udev rules,
# real-time audio permissions, desktop launcher, and PATH binaries.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/adamtdm/ReTraktor/main/install.sh | bash
#   or: sudo ./install.sh
# ==============================================================================

set -euo pipefail

# ANSI styling
BOLD="\033[1m"
GREEN="\033[0;32m"
BLUE="\033[0;34m"
YELLOW="\033[1;33m"
RED="\033[0;31m"
NC="\033[0m"

log_info()    { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $*"; }
log_warn()    { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $*"; }

echo -e "${BOLD}${BLUE}================================================================${NC}"
echo -e "${BOLD}             ReTraktor — Traktor Pro 4 on Linux                 ${NC}"
echo -e "${BOLD}${BLUE}================================================================${NC}"

# Detect OS
OS_NAME="$(uname -s)"
if [ "$OS_NAME" != "Linux" ]; then
    log_warn "This automated 1-command installer is designed for Linux."
    if [ "$OS_NAME" = "Darwin" ]; then
        log_info "Detected macOS. Installing via cargo..."
        cargo install --path crates/retraktor-cli
        log_success "ReTraktor installed to ~/.cargo/bin/retraktor-cli"
        exit 0
    else
        log_error "Unsupported operating system: $OS_NAME"
        exit 1
    fi
fi

# Determine privilege escalations
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        log_error "This script requires superuser privileges to install udev rules and system dependencies."
        log_error "Please run as root or install sudo."
        exit 1
    fi
fi

TARGET_USER="${SUDO_USER:-$USER}"

# 1. Detect Linux distribution and install system dependencies
log_info "Detecting Linux distribution and installing system dependencies..."

if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO_ID="${ID:-unknown}"
    DISTRO_LIKE="${ID_LIKE:-}"
else
    DISTRO_ID="unknown"
    DISTRO_LIKE=""
fi

log_info "Detected distribution: $DISTRO_ID ($DISTRO_LIKE)"

if [[ "$DISTRO_ID" == "ubuntu" || "$DISTRO_ID" == "debian" || "$DISTRO_ID" == "pop" || "$DISTRO_ID" == "linuxmint" || "$DISTRO_LIKE" == *"debian"* ]]; then
    $SUDO apt-get update -qq
    $SUDO DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential pkg-config curl git \
        libudev-dev libusb-1.0-0-dev libhidapi-dev \
        libasound2-dev libpipewire-0.3-dev \
        libx11-dev libxkbcommon-dev libgl1-mesa-dev \
        pipewire pipewire-alsa wireplumber
elif [[ "$DISTRO_ID" == "fedora" || "$DISTRO_ID" == "rhel" || "$DISTRO_ID" == "centos" || "$DISTRO_LIKE" == *"fedora"* ]]; then
    $SUDO dnf install -y \
        gcc gcc-c++ make pkgconf-pkg-config curl git \
        systemd-devel libusb1-devel hidapi-devel \
        alsa-lib-devel pipewire-devel pipewire-alsa wireplumber \
        libxkbcommon-devel libX11-devel mesa-libGL-devel
elif [[ "$DISTRO_ID" == "arch" || "$DISTRO_ID" == "manjaro" || "$DISTRO_LIKE" == *"arch"* ]]; then
    $SUDO pacman -S --needed --noconfirm \
        base-devel pkgconf curl git \
        systemd-libs libusb hidapi \
        alsa-lib pipewire pipewire-alsa wireplumber \
        libxkbcommon libx11 mesa
elif [[ "$DISTRO_ID" == "opensuse"* || "$DISTRO_LIKE" == *"suse"* ]]; then
    $SUDO zypper --non-interactive install \
        gcc gcc-c++ make pkg-config curl git \
        systemd-devel libusb-1_0-devel libhidapi-devel \
        alsa-devel pipewire-devel
else
    log_warn "Unknown Linux distribution. Ensure you have libudev, libusb, hidapi, alsa, and pipewire installed."
fi

# Ensure Cargo is in PATH if installed in ~/.cargo/bin
if [ -d "$HOME/.cargo/bin" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# 2. Check for or install Rust
if ! command -v cargo >/dev/null 2>&1; then
    log_info "Rust toolchain not detected. Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    fi
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# 3. Build ReTraktor
TEMP_DIR=""
cleanup() {
    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR"
    fi
}
trap cleanup EXIT

SCRIPT_DIR=""
if [ -n "${BASH_SOURCE[0]:-}" ]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")"
fi
BIN_DIR=""

# Check if we are running from inside the cloned repo
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/Cargo.toml" ] && [ -d "$SCRIPT_DIR/crates" ]; then
    log_info "Building ReTraktor from local repository..."
    cd "$SCRIPT_DIR"
    cargo build --workspace --release
    BIN_DIR="$SCRIPT_DIR/target/release"
else
    log_info "Fetching latest ReTraktor from GitHub..."
    TEMP_DIR="$(mktemp -d)"
    git clone --depth 1 https://github.com/adamtdm/ReTraktor.git "$TEMP_DIR/ReTraktor"
    cd "$TEMP_DIR/ReTraktor"
    cargo build --workspace --release
    BIN_DIR="$TEMP_DIR/ReTraktor/target/release"
    SCRIPT_DIR="$TEMP_DIR/ReTraktor"
fi

# 4. Packaging & Installation
# If dpkg-deb is available (Ubuntu, Debian, Pop!_OS), build and install a real .deb package
if command -v dpkg-deb >/dev/null 2>&1; then
    log_info "Building and installing native Debian package (.deb)..."
    ARCH="$(uname -m)"
    if [ "$ARCH" = "x86_64" ]; then
        DEB_ARCH="amd64"
    elif [ "$ARCH" = "aarch64" ]; then
        DEB_ARCH="arm64"
    else
        DEB_ARCH="$ARCH"
    fi
    VERSION="0.1.0"
    DEB_STAGE="$(mktemp -d)"
    DEB_ROOT="$DEB_STAGE/retraktor_${VERSION}_${DEB_ARCH}"

    mkdir -p "$DEB_ROOT/DEBIAN"
    mkdir -p "$DEB_ROOT/usr/bin"
    mkdir -p "$DEB_ROOT/lib/udev/rules.d"
    mkdir -p "$DEB_ROOT/etc/udev/rules.d"
    mkdir -p "$DEB_ROOT/etc/security/limits.d"
    mkdir -p "$DEB_ROOT/usr/share/applications"
    mkdir -p "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps"

    install -m 0755 "$BIN_DIR/retraktor-gui" "$DEB_ROOT/usr/bin/retraktor-gui"
    install -m 0755 "$BIN_DIR/retraktor-cli" "$DEB_ROOT/usr/bin/retraktor-cli"
    install -m 0755 "$SCRIPT_DIR/assets/retraktor-wrapper.sh" "$DEB_ROOT/usr/bin/retraktor"
    install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-ni.rules" "$DEB_ROOT/lib/udev/rules.d/60-retraktor-controllers.rules"
    install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-ni.rules" "$DEB_ROOT/lib/udev/rules.d/99-retraktor-ni.rules"
    install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-ni.rules" "$DEB_ROOT/etc/udev/rules.d/60-retraktor-controllers.rules"
    install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-ni.rules" "$DEB_ROOT/etc/udev/rules.d/99-retraktor-ni.rules"
    install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-realtime.conf" "$DEB_ROOT/etc/security/limits.d/99-retraktor-realtime.conf"
    install -m 0644 "$SCRIPT_DIR/assets/retraktor.desktop" "$DEB_ROOT/usr/share/applications/retraktor.desktop"
    install -m 0644 "$SCRIPT_DIR/assets/retraktor.svg" "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps/retraktor.svg"

    cat > "$DEB_ROOT/DEBIAN/control" << EOF
Package: retraktor
Version: ${VERSION}
Section: sound
Priority: optional
Architecture: ${DEB_ARCH}
Depends: libasound2, libusb-1.0-0, libhidapi-hidraw0 | libhidapi-libusb0, libgl1 | libglvnd0, libx11-6
Recommends: pipewire, wireplumber
Maintainer: Adam Meek <adamtdm@users.noreply.github.com>
Description: Clean-room Linux re-implementation of Traktor Pro 4
 ReTraktor provides complete 4-deck DJ mixing, reactive CSI 1.0 QML scripting,
 .nml/.tsi library interoperability, hardware-accelerated 60 FPS waveforms GUI,
 and Traktor Kontrol & MX2 HID/USB hardware support.
EOF

    install -m 0755 "$SCRIPT_DIR/debian/postinst" "$DEB_ROOT/DEBIAN/postinst"
    install -m 0755 "$SCRIPT_DIR/debian/postrm" "$DEB_ROOT/DEBIAN/postrm"

    DEB_OUT="/tmp/retraktor_${VERSION}_${DEB_ARCH}.deb"
    dpkg-deb --build "$DEB_ROOT" "$DEB_OUT"
    rm -rf "$DEB_STAGE"

    $SUDO apt-get install -y --reinstall "$DEB_OUT" || $SUDO dpkg -i "$DEB_OUT"
    # Also save a copy to the invoking user's home directory so they have the .deb file locally
    if [ -n "${HOME:-}" ] && [ -d "$HOME" ]; then
        cp "$DEB_OUT" "$HOME/retraktor_${VERSION}_${DEB_ARCH}.deb" 2>/dev/null || true
    fi
    cp "$DEB_OUT" "./retraktor_${VERSION}_${DEB_ARCH}.deb" 2>/dev/null || true
else
    log_info "Installing binaries and system configuration directly..."
    $SUDO install -m 0755 "$BIN_DIR/retraktor-gui" /usr/local/bin/retraktor-gui
    $SUDO install -m 0755 "$BIN_DIR/retraktor-cli" /usr/local/bin/retraktor-cli
    $SUDO install -m 0755 "$SCRIPT_DIR/assets/retraktor-wrapper.sh" /usr/local/bin/retraktor

    $SUDO install -d -m 0755 /etc/udev/rules.d
    $SUDO install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-ni.rules" /etc/udev/rules.d/99-retraktor-ni.rules

    $SUDO install -d -m 0755 /etc/security/limits.d
    $SUDO install -m 0644 "$SCRIPT_DIR/assets/99-retraktor-realtime.conf" /etc/security/limits.d/99-retraktor-realtime.conf

    $SUDO install -d -m 0755 /usr/share/applications
    $SUDO install -m 0644 "$SCRIPT_DIR/assets/retraktor.desktop" /usr/share/applications/retraktor.desktop

    $SUDO install -d -m 0755 /usr/share/icons/hicolor/scalable/apps
    $SUDO install -m 0644 "$SCRIPT_DIR/assets/retraktor.svg" /usr/share/icons/hicolor/scalable/apps/retraktor.svg
fi

# Ensure both /usr/local/bin and /usr/bin have the active retraktor, retraktor-gui, and retraktor-cli binaries
$SUDO install -m 0755 "$BIN_DIR/retraktor-gui" /usr/local/bin/retraktor-gui 2>/dev/null || true
$SUDO install -m 0755 "$BIN_DIR/retraktor-gui" /usr/bin/retraktor-gui 2>/dev/null || true
$SUDO install -m 0755 "$BIN_DIR/retraktor-cli" /usr/local/bin/retraktor-cli 2>/dev/null || true
$SUDO install -m 0755 "$BIN_DIR/retraktor-cli" /usr/bin/retraktor-cli 2>/dev/null || true
$SUDO install -m 0755 "$SCRIPT_DIR/assets/retraktor-wrapper.sh" /usr/local/bin/retraktor 2>/dev/null || true
$SUDO install -m 0755 "$SCRIPT_DIR/assets/retraktor-wrapper.sh" /usr/bin/retraktor 2>/dev/null || true

# 5. User group enrollment & udev trigger
$SUDO groupadd -f plugdev || true
$SUDO groupadd -f audio || true
if [ -n "$TARGET_USER" ]; then
    log_info "Adding user '$TARGET_USER' to 'plugdev' and 'audio' groups..."
    $SUDO usermod -aG plugdev,audio "$TARGET_USER" || true
fi

if command -v udevadm >/dev/null 2>&1; then
    $SUDO udevadm control --reload-rules || true
    $SUDO udevadm trigger || true
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    $SUDO update-desktop-database /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    $SUDO gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
fi

echo ""
log_success "ReTraktor installation completed successfully!"
echo ""
echo -e "${BOLD}Installed components:${NC}"
echo -e "  • Desktop GUI:     ${GREEN}/usr/local/bin/retraktor${NC} (native 60 FPS Traktor Pro 4 interface)"
echo -e "  • CLI Utility:     ${GREEN}/usr/local/bin/retraktor-cli${NC}"
echo -e "  • NI udev rules:   ${GREEN}/etc/udev/rules.d/99-retraktor-ni.rules${NC}"
echo -e "  • Audio limits:    ${GREEN}/etc/security/limits.d/99-retraktor-realtime.conf${NC}"
echo -e "  • Desktop Launcher:${GREEN}/usr/share/applications/retraktor.desktop${NC}"
echo ""
echo -e "${BOLD}To start using ReTraktor:${NC}"
echo -e "  Simply run:  ${GREEN}retraktor${NC}  or click the ${BOLD}ReTraktor${NC} icon in your application launcher."
echo -e "  Inspect NML: ${CYAN}retraktor-cli inspect-nml <collection.nml>${NC}"
echo -e "  Inspect TSI: ${CYAN}retraktor-cli inspect-tsi <mapping.tsi>${NC}"
echo -e "  Run Doctor:  ${CYAN}retraktor-cli doctor${NC}"
echo ""
if [ -n "$TARGET_USER" ]; then
    echo -e "${YELLOW}[NOTE]${NC} If you were just added to the 'audio'/'plugdev' groups, please log out and back in once for permissions to take full effect."
fi
