#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════════
#  Zobite Tunnel Installer
#  Install server or client with one command:
#
#  Server: curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | bash -s server
#  Client: curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | bash -s client
#  Both:   curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | bash -s all
# ═══════════════════════════════════════════════════════════════════

REPO="Zobite/zo-tunnel"
INSTALL_DIR="/usr/local/bin"
COMPONENT="${1:-client}"   # client, server, or all

# ─── Colors ───
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}▸${NC} $*"; }
ok()    { echo -e "${GREEN}✅${NC} $*"; }
warn()  { echo -e "${YELLOW}⚠️${NC}  $*"; }
fail()  { echo -e "${RED}❌${NC} $*"; exit 1; }

echo ""
echo -e "${BLUE}╔══════════════════════════════════════╗${NC}"
echo -e "${BLUE}║        Zobite Tunnel Installer               ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════╝${NC}"
echo ""

# ─── Detect OS & Arch ───
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)  OS_LABEL="linux" ;;
    Darwin) OS_LABEL="darwin" ;;
    *)      fail "Unsupported OS: $OS (need Linux or macOS)" ;;
esac

case "$ARCH" in
    x86_64|amd64)   ARCH_LABEL="amd64" ;;
    aarch64|arm64)  ARCH_LABEL="arm64" ;;
    *)              fail "Unsupported architecture: $ARCH" ;;
esac

TARGET="${OS_LABEL}-${ARCH_LABEL}"
info "Detected: ${OS} ${ARCH} → ${TARGET}"

# ─── Find latest version ───
info "Finding latest release..."

if command -v curl &>/dev/null; then
    DOWNLOAD="curl -sSL"
    DOWNLOAD_OUT="curl -sSL -o"
elif command -v wget &>/dev/null; then
    DOWNLOAD="wget -qO-"
    DOWNLOAD_OUT="wget -qO"
else
    fail "Need curl or wget"
fi

# Get latest release tag
LATEST=$($DOWNLOAD "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/' | head -1)

if [ -z "$LATEST" ]; then
    warn "Could not detect latest version, using v0.1.0"
    LATEST="v0.1.0"
fi

info "Latest version: ${LATEST}"

# ─── Download & Install ───
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

install_binary() {
    local binary="$1"
    local url="https://github.com/${REPO}/releases/download/${LATEST}/zobite-tunnel-${binary}-${LATEST}-${TARGET}.tar.gz"

    info "Downloading zobite-tunnel-${binary}..."
    $DOWNLOAD_OUT "$TMP_DIR/${binary}.tar.gz" "$url" 2>/dev/null || {
        fail "Download failed: $url"
    }

    tar -xzf "$TMP_DIR/${binary}.tar.gz" -C "$TMP_DIR" 2>/dev/null || {
        fail "Extract failed — corrupt archive?"
    }

    # Install to /usr/local/bin
    if [ -w "$INSTALL_DIR" ]; then
        cp "$TMP_DIR/zobite-tunnel-${binary}" "$INSTALL_DIR/"
    else
        info "Need sudo to install to $INSTALL_DIR"
        sudo cp "$TMP_DIR/zobite-tunnel-${binary}" "$INSTALL_DIR/"
    fi

    chmod +x "$INSTALL_DIR/zobite-tunnel-${binary}"
    ok "Installed zobite-tunnel-${binary} → ${INSTALL_DIR}/zobite-tunnel-${binary}"
}

case "$COMPONENT" in
    client)
        install_binary "client"
        ;;
    server)
        install_binary "server"
        ;;
    all)
        install_binary "client"
        install_binary "server"
        ;;
    *)
        fail "Unknown component: $COMPONENT (use: client, server, or all)"
        ;;
esac

# ─── Verify ───
echo ""
echo -e "${GREEN}═══════════════════════════════════════${NC}"
echo -e "${GREEN}  Installation complete!${NC}"
echo -e "${GREEN}═══════════════════════════════════════${NC}"
echo ""

if [ "$COMPONENT" = "client" ] || [ "$COMPONENT" = "all" ]; then
    echo "  Client usage:"
    echo "    zobite-tunnel-client --server YOUR_VPS:7000 --local localhost:3000 --id app --token SECRET"
    echo ""
fi

if [ "$COMPONENT" = "server" ] || [ "$COMPONENT" = "all" ]; then
    echo "  Server usage:"
    echo "    zobite-tunnel-server --token SECRET"
    echo ""
    echo "  Or with systemd (Linux):"
    echo "    sudo zobite-tunnel-server --token SECRET &"
    echo ""
fi

echo "  Docs: https://github.com/${REPO}"
echo ""
