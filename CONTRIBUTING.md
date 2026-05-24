# Contributing to Zo Tunnel

Thank you for your interest in Zo Tunnel! 🚀

This document provides guidelines on how to contribute to the project.

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

## 📝 Contribution Workflow

### 1. Create an Issue first

Before writing code, create or comment on a related issue:
- **Bug?** → Create a [Bug Report](https://github.com/Zobite/zo-tunnel/issues/new?template=bug_report.md)
- **New feature?** → Create a [Feature Request](https://github.com/Zobite/zo-tunnel/issues/new?template=feature_request.md)

### 2. Fork & Branch

```bash
# Fork the repo on GitHub
# Clone your fork locally
git clone https://github.com/<your-username>/zo-tunnel.git

# Create a new branch
git checkout -b fix/short-description
```

**Branch naming conventions:**
- `fix/description` — bug fix
- `feat/description` — new feature
- `docs/description` — documentation update
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
docs: update README with new default ports
refactor: simplify rate limiter logic
test: add routing extraction tests
```

### 5. Create a Pull Request

- Push your branch to your fork
- Create a PR targeting the `main` branch of the original repo
- Fill in the PR template completely
- Wait for CI to pass (tests + clippy)
- A maintainer will review and provide feedback

## 🏗️ Project Structure

```
zo-tunnel/
├── crates/
│   ├── zo-tunnel-protocol/     # Shared protocol (messages, encoding)
│   ├── zo-tunnel-server/       # Server binary
│   └── zo-tunnel-client/       # Client binary
├── web/                        # Dashboard UI
├── configs/                    # Sample YAML configs
├── scripts/                    # Build, install, test scripts
└── .github/workflows/          # CI/CD
```

## 🧪 Testing

```bash
# Unit tests
cargo test --workspace

# E2E test (requires release build first)
cargo build --release
bash scripts/e2e_test.sh
```

## 📋 Coding Style

- **Formatter:** `cargo fmt` (uses rustfmt defaults)
- **Linter:** `cargo clippy -- -D warnings`
- **Error handling:** Use `anyhow::Result` + `.context("description")`
- **Logging:** Use `tracing::{info, debug, warn, error}`
- **Async:** All I/O must be async (tokio)

## ❓ Questions?

If you have any questions, please create a [Discussion](https://github.com/Zobite/zo-tunnel/discussions) or open an Issue.

Thank you for contributing! ❤️
