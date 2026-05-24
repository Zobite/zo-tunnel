#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════════
#  Zo Tunnel Installer
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
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${BLUE}▸${NC} $*"; }
ok()    { echo -e "${GREEN}✅${NC} $*"; }
warn()  { echo -e "${YELLOW}⚠️${NC}  $*"; }
fail()  { echo -e "${RED}❌${NC} $*"; exit 1; }

echo ""
echo -e "${BLUE}╔══════════════════════════════════════╗${NC}"
echo -e "${BLUE}║        Zo Tunnel Installer               ║${NC}"
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
LATEST=$($DOWNLOAD "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/' | head -1 || true)
LATEST="${LATEST:-}"

# ─── Download & Install ───
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

# Track which binaries to install
BINARIES=()
case "$COMPONENT" in
    client) BINARIES=("client") ;;
    server) BINARIES=("server") ;;
    all)    BINARIES=("client" "server") ;;
    *)      fail "Unknown component: $COMPONENT (use: client, server, or all)" ;;
esac

# Try downloading pre-built binaries from GitHub releases
DOWNLOAD_OK=true

if [ -n "$LATEST" ]; then
    info "Latest release: ${LATEST}"

    for binary in "${BINARIES[@]}"; do
        local_url="https://github.com/${REPO}/releases/download/${LATEST}/zo-tunnel-${binary}-${LATEST}-${TARGET}.tar.gz"
        info "Downloading zo-tunnel-${binary}..."

        if curl -fsSL "$local_url" -o "$TMP_DIR/${binary}.tar.gz" 2>/dev/null; then
            if tar -xzf "$TMP_DIR/${binary}.tar.gz" -C "$TMP_DIR" 2>/dev/null && [ -f "$TMP_DIR/zo-tunnel-${binary}" ]; then
                # Install to /usr/local/bin
                if [ -w "$INSTALL_DIR" ]; then
                    cp "$TMP_DIR/zo-tunnel-${binary}" "$INSTALL_DIR/"
                else
                    info "Need sudo to install to $INSTALL_DIR"
                    sudo cp "$TMP_DIR/zo-tunnel-${binary}" "$INSTALL_DIR/"
                fi
                chmod +x "$INSTALL_DIR/zo-tunnel-${binary}"
                ok "Installed zo-tunnel-${binary} → ${INSTALL_DIR}/zo-tunnel-${binary}"
            else
                warn "Failed to extract zo-tunnel-${binary}"
                DOWNLOAD_OK=false
            fi
        else
            warn "Pre-built binary not available for zo-tunnel-${binary} (${TARGET})"
            DOWNLOAD_OK=false
        fi
    done
else
    info "No GitHub release found"
    DOWNLOAD_OK=false
fi

# ─── Fallback: build from source ───
if [ "$DOWNLOAD_OK" = false ]; then
    warn "No pre-built binary available — building from source..."

    # Find or clone the repo
    REPO_DIR=""
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd 2>/dev/null || pwd)"
    SCRIPT_PARENT="$(dirname "$SCRIPT_DIR")"

    if [ -f "Cargo.toml" ] && grep -q "zo-tunnel" "Cargo.toml" 2>/dev/null; then
        REPO_DIR="$(pwd)"
    elif [ -f "$SCRIPT_PARENT/Cargo.toml" ] && grep -q "zo-tunnel" "$SCRIPT_PARENT/Cargo.toml" 2>/dev/null; then
        REPO_DIR="$SCRIPT_PARENT"
    else
        if ! command -v git &>/dev/null; then
            fail "git is required to clone the repository. Install it first."
        fi
        info "Cloning repository..."
        git clone --depth 1 "https://github.com/${REPO}.git" "$TMP_DIR/zo-tunnel"
        REPO_DIR="$TMP_DIR/zo-tunnel"
    fi

    # Check for cargo/rustc
    if ! command -v cargo &>/dev/null; then
        info "Rust not found — installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    for binary in "${BINARIES[@]}"; do
        info "Building zo-tunnel-${binary} (this may take a few minutes)..."
        cargo build --release -p "zo-tunnel-${binary}" --manifest-path "$REPO_DIR/Cargo.toml"

        BUILT_BINARY="$REPO_DIR/target/release/zo-tunnel-${binary}"
        if [ ! -f "$BUILT_BINARY" ]; then
            fail "Build failed — binary not found at $BUILT_BINARY"
        fi

        if [ -w "$INSTALL_DIR" ]; then
            cp "$BUILT_BINARY" "$INSTALL_DIR/zo-tunnel-${binary}"
        else
            info "Need sudo to install to $INSTALL_DIR"
            sudo cp "$BUILT_BINARY" "$INSTALL_DIR/zo-tunnel-${binary}"
        fi
        chmod +x "$INSTALL_DIR/zo-tunnel-${binary}"
        ok "Built and installed zo-tunnel-${binary} → ${INSTALL_DIR}/zo-tunnel-${binary}"
    done
fi

# ─── Verify ───
echo ""
echo -e "${GREEN}═══════════════════════════════════════${NC}"
echo -e "${GREEN}  Installation complete!${NC}"
echo -e "${GREEN}═══════════════════════════════════════${NC}"
echo ""

for binary in "${BINARIES[@]}"; do
    if command -v "zo-tunnel-${binary}" &>/dev/null; then
        VERSION=$("zo-tunnel-${binary}" --version 2>/dev/null || echo "unknown")
        ok "zo-tunnel-${binary} (${VERSION}) is ready"
    else
        warn "zo-tunnel-${binary} installed but not in PATH — add ${INSTALL_DIR} to your PATH"
    fi
done

echo ""

if [ "$COMPONENT" = "client" ] || [ "$COMPONENT" = "all" ]; then
    echo "  Client usage:"
    echo -e "    ${CYAN}zo-tunnel-client --server YOUR_VPS:6200${NC} \\"
    echo -e "    ${CYAN}  --local localhost:3000 --id my-app${NC} \\"
    echo -e "    ${CYAN}  --token YOUR_TOKEN${NC}"
    echo ""
fi

if [ "$COMPONENT" = "server" ] || [ "$COMPONENT" = "all" ]; then
    echo "  Server setup (port mode — simplest):"
    echo -e "    ${CYAN}zo-tunnel-server setup${NC}"
    echo ""
    echo "  Or with subdomain mode:"
    echo -e "    ${CYAN}zo-tunnel-server setup --domain YOUR_DOMAIN${NC}"
    echo ""
    echo "  Start server:"
    echo -e "    ${CYAN}zo-tunnel-server start${NC}"
    echo ""
fi

echo "  Docs: https://github.com/${REPO}"
echo ""
