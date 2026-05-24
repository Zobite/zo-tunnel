#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════
#  Zobite Tunnel — Release Script
#  Usage: ./scripts/release.sh
#
#  This script will:
#    1. Detect the current version from Cargo.toml
#    2. Let you choose a version bump (patch / minor / major / custom)
#    3. Update version in all Cargo.toml files + Homebrew formula
#    4. Commit, tag, and push → triggers GitHub Actions release
# ═══════════════════════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# ── Colors ──
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()  { echo -e "${BLUE}ℹ${NC}  $*"; }
ok()    { echo -e "${GREEN}✅${NC} $*"; }
warn()  { echo -e "${YELLOW}⚠️${NC}  $*"; }
err()   { echo -e "${RED}❌${NC} $*" >&2; }

# ── Check we're in git repo ──
if ! git -C "$PROJECT_DIR" rev-parse --is-inside-work-tree &>/dev/null; then
    err "Không tìm thấy git repository trong $PROJECT_DIR"
    exit 1
fi

# ── Check for uncommitted changes ──
if ! git -C "$PROJECT_DIR" diff --quiet HEAD 2>/dev/null; then
    warn "Có thay đổi chưa commit. Tiếp tục? (y/N)"
    read -r answer
    if [[ ! "$answer" =~ ^[Yy]$ ]]; then
        info "Hủy release."
        exit 0
    fi
fi

# ── Get current version from first crate Cargo.toml ──
get_current_version() {
    grep -m1 '^version' "$PROJECT_DIR/crates/zobite-tunnel-server/Cargo.toml" \
        | sed 's/version = "\(.*\)"/\1/'
}

CURRENT_VERSION=$(get_current_version)
if [[ -z "$CURRENT_VERSION" ]]; then
    err "Không thể đọc version hiện tại từ Cargo.toml"
    exit 1
fi

# ── Parse semver ──
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

BUMP_PATCH="$MAJOR.$MINOR.$((PATCH + 1))"
BUMP_MINOR="$MAJOR.$((MINOR + 1)).0"
BUMP_MAJOR="$((MAJOR + 1)).0.0"

# ── Display menu ──
echo ""
echo -e "${BOLD}╔══════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║     🚀 Zobite Tunnel — Release Tool      ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════╝${NC}"
echo ""
echo -e "  📦 Version hiện tại: ${CYAN}${BOLD}v${CURRENT_VERSION}${NC}"
echo ""
echo -e "  Chọn version mới:"
echo ""
echo -e "  ${GREEN}1)${NC} Patch  → ${BOLD}v${BUMP_PATCH}${NC}   (bug fixes, nhỏ)"
echo -e "  ${GREEN}2)${NC} Minor  → ${BOLD}v${BUMP_MINOR}${NC}   (tính năng mới)"
echo -e "  ${GREEN}3)${NC} Major  → ${BOLD}v${BUMP_MAJOR}${NC}   (breaking changes)"
echo -e "  ${GREEN}4)${NC} Custom → ${BOLD}nhập tay${NC}"
echo ""
echo -ne "  👉 Chọn (1/2/3/4): "
read -r choice

case "$choice" in
    1) NEW_VERSION="$BUMP_PATCH" ;;
    2) NEW_VERSION="$BUMP_MINOR" ;;
    3) NEW_VERSION="$BUMP_MAJOR" ;;
    4)
        echo -ne "  Nhập version (vd: 1.2.3): "
        read -r NEW_VERSION
        # Validate semver format
        if [[ ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            err "Version không hợp lệ: $NEW_VERSION (phải là x.y.z)"
            exit 1
        fi
        ;;
    *)
        err "Lựa chọn không hợp lệ: $choice"
        exit 1
        ;;
esac

echo ""
echo -e "  ${YELLOW}${BOLD}v${CURRENT_VERSION}${NC} → ${GREEN}${BOLD}v${NEW_VERSION}${NC}"
echo ""
echo -ne "  Xác nhận release ${BOLD}v${NEW_VERSION}${NC}? (y/N): "
read -r confirm
if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
    info "Hủy release."
    exit 0
fi

echo ""
info "Bắt đầu release v${NEW_VERSION}..."
echo ""

# ═══════════════════════════════════════════════════════════════
#  Step 1: Update Cargo.toml versions
# ═══════════════════════════════════════════════════════════════
CARGO_FILES=(
    "$PROJECT_DIR/crates/zobite-tunnel-protocol/Cargo.toml"
    "$PROJECT_DIR/crates/zobite-tunnel-server/Cargo.toml"
    "$PROJECT_DIR/crates/zobite-tunnel-client/Cargo.toml"
)

for file in "${CARGO_FILES[@]}"; do
    if [[ -f "$file" ]]; then
        sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" "$file"
        ok "Cập nhật $(basename "$(dirname "$file")")/Cargo.toml → v${NEW_VERSION}"
    else
        warn "Không tìm thấy: $file"
    fi
done

# ═══════════════════════════════════════════════════════════════
#  Step 2: Update Homebrew formula
# ═══════════════════════════════════════════════════════════════
FORMULA="$PROJECT_DIR/Formula/zobite-tunnel.rb"
if [[ -f "$FORMULA" ]]; then
    sed -i "s/version \"$CURRENT_VERSION\"/version \"$NEW_VERSION\"/" "$FORMULA"
    ok "Cập nhật Formula/zobite-tunnel.rb → v${NEW_VERSION}"
fi

# ═══════════════════════════════════════════════════════════════
#  Step 3: Update Cargo.lock (by running cargo check)
# ═══════════════════════════════════════════════════════════════
info "Cập nhật Cargo.lock..."
(cd "$PROJECT_DIR" && cargo check --quiet 2>/dev/null) || true
ok "Cargo.lock đã cập nhật"

# ═══════════════════════════════════════════════════════════════
#  Step 4: Git commit + tag + push
# ═══════════════════════════════════════════════════════════════
echo ""
info "Git commit & tag..."

cd "$PROJECT_DIR"
git add -A
git commit -m "release: v${NEW_VERSION}" --allow-empty

TAG="v${NEW_VERSION}"
git tag -a "$TAG" -m "Release ${TAG}"
ok "Tạo tag: ${TAG}"

echo ""
info "Push lên remote..."
git push origin main --follow-tags 2>/dev/null || git push origin "$(git branch --show-current)" --follow-tags

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║       🎉 Release v${NEW_VERSION} hoàn tất!          ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════╝${NC}"
echo ""
echo -e "  📌 Tag:      ${CYAN}${TAG}${NC}"
echo -e "  📦 Version:  ${GREEN}v${NEW_VERSION}${NC}"
echo ""
echo -e "  GitHub Actions sẽ tự động:"
echo -e "    • Build binaries (linux/macos × amd64/arm64)"
echo -e "    • Push Docker image lên GHCR"
echo -e "    • Tạo GitHub Release + upload artifacts"
echo -e "    • Cập nhật Homebrew formula"
echo ""
echo -e "  🔗 Theo dõi tại: ${BLUE}https://github.com/Zobite/zo-tunnel/actions${NC}"
echo ""
