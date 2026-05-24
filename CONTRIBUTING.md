# Contributing to Zo Tunnel

Cảm ơn bạn quan tâm đến Zo Tunnel! 🚀

Tài liệu này hướng dẫn cách đóng góp vào dự án.

## 🚀 Quick Start

### Prerequisites

- [Rust 1.75+](https://rustup.rs/)
- Git

### Setup

```bash
# Fork repo trên GitHub, rồi clone về
git clone https://github.com/<your-username>/zo-tunnel.git
cd zo-tunnel

# Build
cargo build

# Chạy tests
cargo test --workspace

# Chạy clippy (linter)
cargo clippy --workspace -- -D warnings
```

## 📝 Quy Trình Đóng Góp

### 1. Tạo Issue trước

Trước khi code, hãy tạo hoặc comment vào issue liên quan:
- **Bug?** → Tạo [Bug Report](https://github.com/Zobite/zo-tunnel/issues/new?template=bug_report.md)
- **Feature mới?** → Tạo [Feature Request](https://github.com/Zobite/zo-tunnel/issues/new?template=feature_request.md)

### 2. Fork & Branch

```bash
# Fork repo trên GitHub
# Clone fork về máy
git clone https://github.com/<your-username>/zo-tunnel.git

# Tạo branch mới
git checkout -b fix/ten-mieu-ta-ngan
```

**Quy tắc đặt tên branch:**
- `fix/mieu-ta` — sửa bug
- `feat/mieu-ta` — tính năng mới
- `docs/mieu-ta` — cập nhật documentation
- `refactor/mieu-ta` — refactor code

### 3. Code

- Tuân theo Rust coding conventions
- Chạy `cargo fmt` trước khi commit
- Chạy `cargo clippy --workspace -- -D warnings` — không được có warning
- Thêm tests cho code mới nếu có thể

### 4. Commit

Sử dụng [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add WebSocket tunnel support
fix: handle client disconnect during auth
docs: update README with new default ports
refactor: simplify rate limiter logic
test: add routing extraction tests
```

### 5. Tạo Pull Request

- Push branch lên fork
- Tạo PR về `main` branch của repo gốc
- Điền đầy đủ PR template
- Đợi CI pass (tests + clippy)
- Maintainer sẽ review và phản hồi

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

# E2E test (cần build release trước)
cargo build --release
bash scripts/e2e_test.sh
```

## 📋 Coding Style

- **Formatter:** `cargo fmt` (đặt theo rustfmt defaults)
- **Linter:** `cargo clippy -- -D warnings`
- **Error handling:** Dùng `anyhow::Result` + `.context("mô tả")`
- **Logging:** Dùng `tracing::{info, debug, warn, error}`
- **Async:** Tất cả I/O phải async (tokio)

## ❓ Câu Hỏi?

Nếu bạn có thắc mắc, hãy tạo một [Discussion](https://github.com/Zobite/zo-tunnel/discussions) hoặc mở Issue.

Cảm ơn bạn đã đóng góp! ❤️
