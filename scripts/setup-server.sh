#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════════
#  Zo Tunnel Server — One-line setup for Linux VPS
#
#  Usage:
#    curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/setup-server.sh | bash
#
#  With custom token:
#    curl -sSL ... | ZO_TOKEN=my-secret bash
#
#  This script will:
#    1. Download the latest zo-tunnel-server binary
#    2. Create a systemd service
#    3. Open firewall ports
#    4. Start the server
# ═══════════════════════════════════════════════════════════════════

REPO="Zobite/zo-tunnel"
INSTALL_DIR="/usr/local/bin"
TOKEN="${ZO_TOKEN:-$(openssl rand -hex 24)}"
CONTROL_PORT="${ZO_CONTROL_PORT:-7000}"
PUBLIC_PORT="${ZO_PUBLIC_PORT:-8080}"
DASHBOARD_PORT="${ZO_DASHBOARD_PORT:-9000}"

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
echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     Zo Tunnel Server Setup               ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""

# ─── Check root ───
if [ "$EUID" -ne 0 ]; then
    fail "Please run as root: sudo bash or curl ... | sudo bash"
fi

# ─── Detect arch ───
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)  TARGET="linux-amd64" ;;
    aarch64|arm64) TARGET="linux-arm64" ;;
    *)             fail "Unsupported arch: $ARCH" ;;
esac

info "Platform: Linux ${ARCH} → ${TARGET}"

# ─── Download latest binary ───
info "Finding latest release..."
LATEST=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/' | head -1)
LATEST="${LATEST:-v0.1.0}"
info "Version: ${LATEST}"

URL="https://github.com/${REPO}/releases/download/${LATEST}/zo-tunnel-server-${LATEST}-${TARGET}.tar.gz"
info "Downloading ${URL}..."

TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

curl -sSL "$URL" -o "$TMP_DIR/server.tar.gz"
tar -xzf "$TMP_DIR/server.tar.gz" -C "$TMP_DIR"
cp "$TMP_DIR/zo-tunnel-server" "$INSTALL_DIR/zo-tunnel-server"
chmod +x "$INSTALL_DIR/zo-tunnel-server"
ok "Installed zo-tunnel-server → ${INSTALL_DIR}"

# ─── Create systemd service ───
info "Creating systemd service..."

cat > /etc/systemd/system/zo-tunnel.service << EOF
[Unit]
Description=Zo Tunnel Tunnel Server
Documentation=https://github.com/${REPO}
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=nobody
Group=nogroup
ExecStart=${INSTALL_DIR}/zo-tunnel-server \\
  --control-port ${CONTROL_PORT} \\
  --public-port ${PUBLIC_PORT} \\
  --dashboard-port ${DASHBOARD_PORT} \\
  --token ${TOKEN}
Restart=always
RestartSec=5
Environment=RUST_LOG=info

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

# Allow binding to low ports
AmbientCapabilities=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
ok "Systemd service created"

# ─── Firewall ───
info "Configuring firewall..."

if command -v ufw &>/dev/null; then
    ufw allow "${CONTROL_PORT}/tcp" >/dev/null 2>&1 || true
    ufw allow "${PUBLIC_PORT}/tcp" >/dev/null 2>&1 || true
    ufw allow "${DASHBOARD_PORT}/tcp" >/dev/null 2>&1 || true
    ufw allow 10000:10100/tcp >/dev/null 2>&1 || true
    ok "UFW rules added"
elif command -v firewall-cmd &>/dev/null; then
    firewall-cmd --permanent --add-port="${CONTROL_PORT}/tcp" >/dev/null 2>&1 || true
    firewall-cmd --permanent --add-port="${PUBLIC_PORT}/tcp" >/dev/null 2>&1 || true
    firewall-cmd --permanent --add-port="${DASHBOARD_PORT}/tcp" >/dev/null 2>&1 || true
    firewall-cmd --permanent --add-port="10000-10100/tcp" >/dev/null 2>&1 || true
    firewall-cmd --reload >/dev/null 2>&1 || true
    ok "firewalld rules added"
else
    warn "No firewall tool found — make sure ports ${CONTROL_PORT}, ${PUBLIC_PORT}, ${DASHBOARD_PORT} are open"
fi

# ─── Start ───
info "Starting Zo Tunnel server..."
systemctl enable --now zo-tunnel
sleep 2

if systemctl is-active --quiet zo-tunnel; then
    ok "Zo Tunnel server is running!"
else
    warn "Service may not have started. Check: journalctl -u zo-tunnel -f"
fi

# ─── Summary ───
VPS_IP=$(curl -s ifconfig.me 2>/dev/null || hostname -I | awk '{print $1}')

echo ""
echo -e "${GREEN}═══════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Zo Tunnel Server is ready!${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Server IP:    ${CYAN}${VPS_IP}${NC}"
echo -e "  Control:      ${CYAN}:${CONTROL_PORT}${NC}"
echo -e "  Public HTTP:  ${CYAN}:${PUBLIC_PORT}${NC}"
echo -e "  Dashboard:    ${CYAN}http://${VPS_IP}:${DASHBOARD_PORT}${NC}"
echo -e "  Auth Token:   ${YELLOW}${TOKEN}${NC}"
echo ""
echo "  ─────────────────────────────────────────────────────"
echo "  Connect from your Mac/Linux:"
echo ""
echo -e "    ${CYAN}curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | bash${NC}"
echo ""
echo -e "    ${CYAN}zo-tunnel-client --server ${VPS_IP}:${CONTROL_PORT} \\${NC}"
echo -e "    ${CYAN}  --local localhost:3000 --id my-app \\${NC}"
echo -e "    ${CYAN}  --token ${TOKEN}${NC}"
echo ""
echo "  ─────────────────────────────────────────────────────"
echo "  Management:"
echo ""
echo "    Status:   systemctl status zo-tunnel"
echo "    Logs:     journalctl -u zo-tunnel -f"
echo "    Restart:  systemctl restart zo-tunnel"
echo "    Stop:     systemctl stop zo-tunnel"
echo ""

# ─── Save token to file for reference ───
echo "${TOKEN}" > /etc/zo-tunnel-token
chmod 600 /etc/zo-tunnel-token
info "Token saved to /etc/zo-tunnel-token"
