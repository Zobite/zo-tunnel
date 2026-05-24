# Contributing to Zo Tunnel

Thank you for your interest in Zo Tunnel! 🚀

## 🚀 Quick Start

### Prerequisites

- [Rust 1.75+](https://rustup.rs/)
- Git

### Setup

```bash
# Fork the repo on GitHub, then clone it
git clone https://github.com/<your-username>/zo-tunnel.git
cd zo-tunnel

# Build
cargo build

# Run tests
cargo test --workspace

# Run clippy (linter)
cargo clippy --workspace -- -D warnings
```

### Local Dev

```bash
# Setup server config (port mode, no domain needed)
make setup-server

# Start server
make run-server

# Connect a test client
make run-client
```

## 📝 Contribution Workflow

### 1. Create an Issue first

Before writing code, create or comment on a related issue:
- **Bug?** → Create a [Bug Report](https://github.com/Zobite/zo-tunnel/issues/new?template=bug_report.md)
- **New feature?** → Create a [Feature Request](https://github.com/Zobite/zo-tunnel/issues/new?template=feature_request.md)

### 2. Fork & Branch

```bash
git clone https://github.com/<your-username>/zo-tunnel.git
git checkout -b fix/short-description
```

**Branch naming:**
- `fix/description` — bug fix
- `feat/description` — new feature
- `docs/description` — documentation
- `refactor/description` — code refactor

### 3. Code

- Follow Rust coding conventions
- Run `cargo fmt` before committing
- Run `cargo clippy --workspace -- -D warnings` — no warnings allowed
- Add tests for new code when possible

### 4. Commit

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add WebSocket tunnel support
fix: handle client disconnect during auth
docs: update README with new architecture
refactor: simplify port allocation logic
test: add subdomain routing tests
```

### 5. Create a Pull Request

- Push your branch to your fork
- Create a PR targeting `main`
- Fill in the PR template
- Wait for CI to pass (tests + clippy)

## 🏗️ Project Structure

```
zo-tunnel/
├── crates/
│   ├── zo-tunnel-protocol/     # Shared protocol (messages, encoding)
│   ├── zo-tunnel-server/       # Server binary
│   │   └── src/
│   │       ├── main.rs         # CLI: setup / start / status
│   │       ├── config.rs       # Config with port & subdomain modes
│   │       ├── server.rs       # Core: control, yamux, port allocation
│   │       ├── proxy.rs        # HTTP proxy (subdomain mode)
│   │       ├── dashboard.rs    # Dashboard API + UI
│   │       ├── registry.rs     # Client registry
│   │       └── metrics.rs      # Metrics + rate limiter
│   └── zo-tunnel-client/       # Client binary
├── web/                        # Dashboard UI (HTML/CSS/JS)
├── configs/                    # Example YAML configs
├── scripts/                    # Install, test scripts
└── .github/                    # Issue templates, PR template
```

## 🧪 Testing

```bash
# Unit tests
cargo test --workspace

# E2E test (requires release build)
cargo build --release
bash scripts/e2e_test.sh
```

## 📋 Coding Style

- **Formatter:** `cargo fmt` (rustfmt defaults)
- **Linter:** `cargo clippy -- -D warnings`
- **Error handling:** `anyhow::Result` + `.context("description")`
- **Logging:** `tracing::{info, debug, warn, error}`
- **Async:** All I/O must be async (tokio)

## ❓ Questions?

Create a [Discussion](https://github.com/Zobite/zo-tunnel/discussions) or open an Issue.

Thank you for contributing! ❤️
