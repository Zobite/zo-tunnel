#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════
#  Zo Tunnel — One-Command Release
#
#  Usage: ./scripts/release.sh
#
#  Tự động 100%:
#    1. Chọn version bump (patch/minor/major)
#    2. Chạy tests + clippy
#    3. Update version trong tất cả Cargo.toml
#    4. Build binaries cho linux + macOS (amd64 + arm64)
#    5. Git commit + tag + push
#    6. Tạo GitHub Release + upload binaries (dùng gh CLI)
#
#  Yêu cầu:
#    - gh CLI đã auth (gh auth login)
#    - gcc-aarch64-linux-gnu (tự cài nếu thiếu)
#    - cargo-zigbuild + zig (tự cài nếu thiếu — dùng cho macOS cross-compile)
# ═══════════════════════════════════════════════════════════════

REPO="Zobite/zo-tunnel"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$PROJECT_DIR/dist"

# ─── Colors ───
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()   { echo -e "${BLUE}▸${NC} $*"; }
ok()     { echo -e "${GREEN}✅${NC} $*"; }
warn()   { echo -e "${YELLOW}⚠️${NC}  $*"; }
fail()   { echo -e "${RED}❌${NC} $*"; exit 1; }
header() { echo -e "\n${BOLD}${CYAN}═══ $* ═══${NC}\n"; }

echo ""
echo -e "${CYAN}╔══════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║       🚀 Zo Tunnel Release               ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════════╝${NC}"
echo ""

# ═══════════════════════════════════════════════════════════════
#  Pre-flight
# ═══════════════════════════════════════════════════════════════

cd "$PROJECT_DIR"

HOST_OS="$(uname -s)"

# Git check
git rev-parse --is-inside-work-tree &>/dev/null || fail "Not a git repo"

# gh CLI check
if ! command -v gh &>/dev/null; then
    if [ "$HOST_OS" = "Darwin" ]; then
        fail "gh CLI not installed. Run: brew install gh && gh auth login"
    else
        fail "gh CLI not installed. Run: apt install gh && gh auth login"
    fi
fi
gh auth status &>/dev/null || fail "gh not authenticated. Run: gh auth login"

# ARM64 cross-compiler (for Linux ARM64 - only needed on Linux host)
if [ "$HOST_OS" = "Linux" ]; then
    if ! command -v aarch64-linux-gnu-gcc &>/dev/null; then
        info "Installing aarch64-linux-gnu-gcc..."
        sudo apt-get update -qq && sudo apt-get install -y -qq gcc-aarch64-linux-gnu >/dev/null 2>&1 \
            || warn "Could not install gcc-aarch64 — Linux ARM64 build will be skipped"
    fi
fi

# Rust targets
rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true
rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true
rustup target add x86_64-apple-darwin 2>/dev/null || true
rustup target add aarch64-apple-darwin 2>/dev/null || true

# Cargo config for Linux ARM64 linker (only needed on Linux host)
if [ "$HOST_OS" = "Linux" ]; then
    mkdir -p "$PROJECT_DIR/.cargo"
    if ! grep -q "aarch64-unknown-linux-gnu" "$PROJECT_DIR/.cargo/config.toml" 2>/dev/null; then
        cat >> "$PROJECT_DIR/.cargo/config.toml" <<'EOF'

[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
EOF
    fi
fi

# cargo-zigbuild (for macOS cross-compile from Linux)
if ! command -v cargo-zigbuild &>/dev/null; then
    info "Installing cargo-zigbuild..."
    cargo install cargo-zigbuild || fail "Could not install cargo-zigbuild"
fi

# zig compiler
if ! command -v zig &>/dev/null; then
    info "Installing zig..."
    if [ "$HOST_OS" = "Darwin" ]; then
        fail "zig not installed. Run: brew install zig"
    else
        ZIG_VERSION="0.13.0"
        ZIG_ARCH="$(uname -m)"
        ZIG_URL="https://ziglang.org/download/${ZIG_VERSION}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}.tar.xz"
        ZIG_DIR="/usr/local/zig"

        TMP_ZIG=$(mktemp -d)
        curl -sSL "$ZIG_URL" -o "$TMP_ZIG/zig.tar.xz"
        sudo mkdir -p "$ZIG_DIR"
        sudo tar -xJf "$TMP_ZIG/zig.tar.xz" -C "$ZIG_DIR" --strip-components=1
        sudo ln -sf "$ZIG_DIR/zig" /usr/local/bin/zig
        rm -rf "$TMP_ZIG"
        command -v zig &>/dev/null || fail "zig installation failed"
        ok "zig $(zig version) installed"
    fi
fi

# ═══════════════════════════════════════════════════════════════
#  Version Selection
# ═══════════════════════════════════════════════════════════════

CURRENT_VERSION=$(grep -m1 '^version' "$PROJECT_DIR/crates/zo-tunnel-server/Cargo.toml" \
    | sed 's/version = "\(.*\)"/\1/')
[ -z "$CURRENT_VERSION" ] && fail "Cannot read version from Cargo.toml"

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

BUMP_PATCH="$MAJOR.$MINOR.$((PATCH + 1))"
BUMP_MINOR="$MAJOR.$((MINOR + 1)).0"
BUMP_MAJOR="$((MAJOR + 1)).0.0"

echo -e "  📦 Current: ${CYAN}${BOLD}v${CURRENT_VERSION}${NC}"
echo ""
echo -e "  ${GREEN}1)${NC} Patch  → ${BOLD}v${BUMP_PATCH}${NC}"
echo -e "  ${GREEN}2)${NC} Minor  → ${BOLD}v${BUMP_MINOR}${NC}"
echo -e "  ${GREEN}3)${NC} Major  → ${BOLD}v${BUMP_MAJOR}${NC}"
echo ""
echo -ne "  Choose (1/2/3): "
read -r choice

case "$choice" in
    1) NEW_VERSION="$BUMP_PATCH" ;;
    2) NEW_VERSION="$BUMP_MINOR" ;;
    3) NEW_VERSION="$BUMP_MAJOR" ;;
    *) fail "Invalid choice" ;;
esac

TAG="v${NEW_VERSION}"
echo ""
echo -e "  ${YELLOW}v${CURRENT_VERSION}${NC} → ${GREEN}${BOLD}${TAG}${NC}"
echo -ne "  Confirm? (y/N): "
read -r confirm
[[ "$confirm" =~ ^[Yy]$ ]] || { info "Cancelled."; exit 0; }

# ═══════════════════════════════════════════════════════════════
#  Step 1: Tests
# ═══════════════════════════════════════════════════════════════
header "Step 1/6 — Tests"

info "cargo test..."
cargo test --workspace --quiet 2>&1 || fail "Tests failed"
ok "Tests passed"

info "cargo clippy..."
cargo clippy --workspace -- -D warnings 2>&1 || fail "Clippy failed"
ok "Clippy passed"

# ═══════════════════════════════════════════════════════════════
#  Step 2: Update Versions
# ═══════════════════════════════════════════════════════════════
header "Step 2/6 — Update Versions"

for crate in zo-tunnel-protocol zo-tunnel-server zo-tunnel-client; do
    FILE="$PROJECT_DIR/crates/$crate/Cargo.toml"
    if [ -f "$FILE" ]; then
        if [ "$HOST_OS" = "Darwin" ]; then
            sed -i "" "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" "$FILE"
        else
            sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" "$FILE"
        fi
        ok "$crate → v${NEW_VERSION}"
    fi
done

# Update Cargo.lock
cargo check --quiet 2>/dev/null || true
ok "Cargo.lock updated"

# ═══════════════════════════════════════════════════════════════
#  Step 3: Build Binaries
# ═══════════════════════════════════════════════════════════════
header "Step 3/6 — Build Binaries"

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

# Linux targets: use regular cargo build
LINUX_TARGETS=(
    "x86_64-unknown-linux-gnu:linux-amd64"
    "aarch64-unknown-linux-gnu:linux-arm64"
)

# macOS targets: use cargo-zigbuild
MACOS_TARGETS=(
    "x86_64-apple-darwin:darwin-amd64"
    "aarch64-apple-darwin:darwin-arm64"
)

BINARIES=("zo-tunnel-server" "zo-tunnel-client")
BUILT_FILES=()

# ─── Build Linux targets ───
for entry in "${LINUX_TARGETS[@]}"; do
    TARGET="${entry%%:*}"
    LABEL="${entry##*:}"

    if [ "$HOST_OS" = "Linux" ]; then
        BUILD_CMD="cargo build"
    else
        BUILD_CMD="cargo zigbuild"
    fi

    info "Building ${BOLD}${LABEL}${NC} (${BUILD_CMD})..."

    if $BUILD_CMD --release --target "$TARGET" 2>&1; then
        for binary in "${BINARIES[@]}"; do
            BIN_PATH="$PROJECT_DIR/target/$TARGET/release/$binary"
            if [ -f "$BIN_PATH" ]; then
                TAR_NAME="${binary}-${TAG}-${LABEL}.tar.gz"
                tar -czf "$DIST_DIR/$TAR_NAME" -C "$(dirname "$BIN_PATH")" "$binary"
                SIZE=$(du -h "$DIST_DIR/$TAR_NAME" | cut -f1)
                ok "${TAR_NAME} (${SIZE})"
                BUILT_FILES+=("$DIST_DIR/$TAR_NAME")
            fi
        done
    else
        warn "Failed ${LABEL} — skipping"
    fi
done

# ─── Build macOS targets ───
for entry in "${MACOS_TARGETS[@]}"; do
    TARGET="${entry%%:*}"
    LABEL="${entry##*:}"

    if [ "$HOST_OS" = "Darwin" ]; then
        BUILD_CMD="cargo build"
    else
        BUILD_CMD="cargo zigbuild"
    fi

    info "Building ${BOLD}${LABEL}${NC} (${BUILD_CMD})..."

    if $BUILD_CMD --release --target "$TARGET" 2>&1; then
        for binary in "${BINARIES[@]}"; do
            BIN_PATH="$PROJECT_DIR/target/$TARGET/release/$binary"
            if [ -f "$BIN_PATH" ]; then
                TAR_NAME="${binary}-${TAG}-${LABEL}.tar.gz"
                tar -czf "$DIST_DIR/$TAR_NAME" -C "$(dirname "$BIN_PATH")" "$binary"
                SIZE=$(du -h "$DIST_DIR/$TAR_NAME" | cut -f1)
                ok "${TAR_NAME} (${SIZE})"
                BUILT_FILES+=("$DIST_DIR/$TAR_NAME")
            fi
        done
    else
        warn "Failed ${LABEL} — skipping"
    fi
done

# Checksums
cd "$DIST_DIR"
sha256sum *.tar.gz > SHA256SUMS.txt 2>/dev/null || shasum -a 256 *.tar.gz > SHA256SUMS.txt
BUILT_FILES+=("$DIST_DIR/SHA256SUMS.txt")
ok "SHA256SUMS.txt"
cd "$PROJECT_DIR"

[ ${#BUILT_FILES[@]} -le 1 ] && fail "No binaries built!"

# ═══════════════════════════════════════════════════════════════
#  Step 4: Git Commit + Tag + Push
# ═══════════════════════════════════════════════════════════════
header "Step 4/6 — Git Push"

git add -A
git commit -m "release: ${TAG}"
git tag -a "$TAG" -m "Release ${TAG}"
git push origin "$(git branch --show-current)" --follow-tags
ok "Pushed ${TAG}"

# ═══════════════════════════════════════════════════════════════
#  Step 5+6: Create GitHub Release + Upload
# ═══════════════════════════════════════════════════════════════
header "Step 5/6 — GitHub Release + Upload"

RELEASE_NOTES="## Install

**Server (Linux VPS):**
\`\`\`bash
curl -sSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | sudo bash -s server
zo-tunnel-server start --domain tunnel.example.com
\`\`\`

**Client (macOS / Linux):**
\`\`\`bash
curl -sSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash
\`\`\`"

gh release create "$TAG" "${BUILT_FILES[@]}" \
    --repo "$REPO" \
    --title "$TAG" \
    --notes "$RELEASE_NOTES"

ok "Release created + binaries uploaded"

# ═══════════════════════════════════════════════════════════════
#  Done!
# ═══════════════════════════════════════════════════════════════
RELEASE_URL="https://github.com/${REPO}/releases/tag/${TAG}"

echo ""
echo -e "${GREEN}╔══════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  🎉 Release ${TAG} complete!                          ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  🔗 ${CYAN}${RELEASE_URL}${NC}"
echo ""
echo -e "  Binaries built:"
echo -e "    • linux-amd64 + linux-arm64   (cargo)"
echo -e "    • darwin-amd64 + darwin-arm64 (cargo-zigbuild)"
echo ""
echo -e "  Install:"
echo -e "    ${CYAN}curl -sSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | bash${NC}"
echo ""
