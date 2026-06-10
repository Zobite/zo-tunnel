#!/bin/bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════════
#  Zo Tunnel — Release (tag & push)
#
#  Usage: ./scripts/release.sh
#
#  Workflow:
#    1. Chọn version bump (patch/minor/major)
#    2. Chạy tests + clippy
#    3. Update version trong tất cả Cargo.toml
#    4. Git commit + tag + push
#    5. GitHub Actions tự build + tạo release
# ═══════════════════════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

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

# Git check
git rev-parse --is-inside-work-tree &>/dev/null || fail "Not a git repo"

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
header "Step 1/3 — Tests"

info "cargo test..."
cargo test --workspace --quiet 2>&1 || fail "Tests failed"
ok "Tests passed"

info "cargo clippy..."
cargo clippy --workspace -- -D warnings 2>&1 || fail "Clippy failed"
ok "Clippy passed"

# ═══════════════════════════════════════════════════════════════
#  Step 2: Update Versions
# ═══════════════════════════════════════════════════════════════
header "Step 2/3 — Update Versions"

HOST_OS="$(uname -s)"

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
#  Step 3: Git Commit + Tag + Push
# ═══════════════════════════════════════════════════════════════
header "Step 3/3 — Git Push"

git add -A
git commit -m "release: ${TAG}"
git tag -a "$TAG" -m "Release ${TAG}"
git push origin "$(git branch --show-current)" --follow-tags
ok "Pushed ${TAG}"

# ═══════════════════════════════════════════════════════════════
#  Done!
# ═══════════════════════════════════════════════════════════════

echo ""
echo -e "${GREEN}╔══════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  🚀 ${TAG} pushed — GitHub Actions building...       ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  📋 Check build:   ${CYAN}https://github.com/Zobite/zo-tunnel/actions${NC}"
echo -e "  📦 Release:       ${CYAN}https://github.com/Zobite/zo-tunnel/releases/tag/${TAG}${NC}"
echo ""
