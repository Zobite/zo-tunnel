.PHONY: build build-server build-client build-all clean test run-server run-client docker

# ── Build ──────────────────────────────────────────────

build: build-all

build-server:
	cargo build --release -p zo-tunnel-server

build-client:
	cargo build --release -p zo-tunnel-client

build-all:
	cargo build --release

# ── Test ───────────────────────────────────────────────

test:
	cargo test --workspace

test-e2e: build-all
	bash scripts/e2e_test.sh

# ── Run (dev) ──────────────────────────────────────────

run-server:
	RUST_LOG=info cargo run -p zo-tunnel-server -- start

run-client:
	RUST_LOG=info cargo run -p zo-tunnel-client -- \
		--server 127.0.0.1:6200 \
		--local localhost:3000 \
		--id my-app

# ── Docker ─────────────────────────────────────────────

docker:
	docker build -t zo-tunnel-server --target server .
	docker build -t zo-tunnel-client --target client .

docker-up:
	docker compose up -d

docker-down:
	docker compose down

# ── Clean ──────────────────────────────────────────────

clean:
	cargo clean

# ── Cross-compile ─────────────────────────────────────

cross-linux-amd64:
	cross build --release --target x86_64-unknown-linux-gnu

cross-linux-arm64:
	cross build --release --target aarch64-unknown-linux-gnu

cross-macos-amd64:
	cross build --release --target x86_64-apple-darwin

cross-macos-arm64:
	cross build --release --target aarch64-apple-darwin

cross-all: cross-linux-amd64 cross-linux-arm64
