#!/bin/bash
set -euo pipefail

# Zo Tunnel Cross-Compile Build Script
# Usage: ./scripts/build.sh [target]
# Targets: linux-amd64, linux-arm64, macos-amd64, macos-arm64, all

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUT_DIR="$PROJECT_DIR/dist"

TARGETS=(
    "x86_64-unknown-linux-gnu:linux-amd64"
    "aarch64-unknown-linux-gnu:linux-arm64"
    "x86_64-apple-darwin:macos-amd64"
    "aarch64-apple-darwin:macos-arm64"
)

build_target() {
    local rust_target="$1"
    local label="$2"
    local dir="$OUT_DIR/$label"

    echo "🔨 Building for $label ($rust_target)..."
    mkdir -p "$dir"

    if command -v cross &> /dev/null; then
        cross build --release --target "$rust_target"
    else
        cargo build --release --target "$rust_target"
    fi

    cp "$PROJECT_DIR/target/$rust_target/release/zo-tunnel-server" "$dir/" 2>/dev/null || true
    cp "$PROJECT_DIR/target/$rust_target/release/zo-tunnel-client" "$dir/" 2>/dev/null || true

    # Create tarball
    (cd "$OUT_DIR" && tar -czf "zo-tunnel-$label.tar.gz" "$label/")
    echo "✅ $label → $OUT_DIR/zo-tunnel-$label.tar.gz"
}

requested="${1:-all}"

if [ "$requested" = "all" ]; then
    for entry in "${TARGETS[@]}"; do
        IFS=':' read -r target label <<< "$entry"
        build_target "$target" "$label" || echo "⚠️  Skipped $label"
    done
elif [ "$requested" = "native" ]; then
    echo "🔨 Building native release..."
    cargo build --release
    echo "✅ Binaries at target/release/zo-tunnel-{server,client}"
else
    for entry in "${TARGETS[@]}"; do
        IFS=':' read -r target label <<< "$entry"
        if [ "$label" = "$requested" ]; then
            build_target "$target" "$label"
            exit 0
        fi
    done
    echo "❌ Unknown target: $requested"
    echo "Available: linux-amd64, linux-arm64, macos-amd64, macos-arm64, all, native"
    exit 1
fi

echo ""
echo "📦 Build complete! Archives in $OUT_DIR/"
ls -lh "$OUT_DIR"/*.tar.gz 2>/dev/null || true
