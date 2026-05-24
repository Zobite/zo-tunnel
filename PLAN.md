# Zobite Zobite Tunnel — Implementation Plan

> **Self-hosted ngrok alternative.**
> Expose any local service to the internet through your own VPS.

---

## 1. Tổng Quan (Overview)

| Mục | Chi tiết |
|---|---|
| **Tên dự án** | Zobite Zobite Tunnel |
| **Mục tiêu** | Cho phép người dùng Internet truy cập service chạy trên máy local (sau NAT/Firewall) thông qua 1 VPS trung gian |
| **Thành phần** | `zobite-tunnel-server` (chạy trên VPS) + `zobite-tunnel-client` (chạy trên máy local) |
| **Ngôn ngữ** | **Rust** — binary tĩnh, zero-cost abstractions, memory safety, hiệu năng ngang C/C++, async I/O mạnh mẽ với Tokio |
| **License** | MIT (hoặc tuỳ chọn) |

---

## 2. Kiến Trúc Tổng Thể (Architecture)

```
┌─────────────────┐          ┌──────────────────────────┐          ┌─────────────────┐
│  Public User     │  HTTP    │     zobite-tunnel-server (VPS)   │  Tunnel  │  zobite-tunnel-client   │
│  (Browser/curl)  │────────▶│                          │◀─────────│  (Local Machine) │
│                  │◀────────│  :80 public listener     │─────────▶│                  │
└─────────────────┘  Response│  :7000 control channel   │  Mux/TCP │  localhost:3000  │
                             │  :9000 dashboard (opt)   │          └─────────────────┘
                             └──────────────────────────┘
```

### Data Flow

```
1. Client ──TCP/WebSocket──▶ Server:7000   (Register + keep-alive)
2. User   ──HTTP──────────▶ Server:80      (Public request)
3. Server ──Tunnel─────────▶ Client        (Forward request bytes)
4. Client ──HTTP───────────▶ localhost:X    (Proxy to local service)
5. Response đi ngược: Local → Client → Server → User
```

---

## 3. Cấu Trúc Thư Mục Dự Kiến (Project Structure)

```
zobite_zobite-tunnel/
├── PLAN.md                  # File này
├── README.md
├── Cargo.toml               # Workspace root
├── Cargo.lock
│
├── crates/
│   ├── zobite-tunnel-server/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs          # Entry point cho zobite-tunnel-server
│   │       ├── server.rs        # Core server logic
│   │       ├── tunnel.rs        # Quản lý tunnel connections
│   │       ├── proxy.rs         # HTTP reverse proxy handler
│   │       ├── registry.rs      # Client registry (map client_id → connection)
│   │       ├── dashboard.rs     # Dashboard API (Phase 3)
│   │       └── metrics.rs       # Metrics collection (Phase 3)
│   │
│   ├── zobite-tunnel-client/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs          # Entry point cho zobite-tunnel-client
│   │       ├── client.rs        # Core client logic
│   │       ├── tunnel.rs        # Tunnel connection handler
│   │       └── proxy.rs         # Local proxy (forward to localhost:X)
│   │
│   └── zobite-tunnel-protocol/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs           # Message frame format, reader/writer, shared types
│
├── configs/
│   ├── server.yaml          # Server config mẫu
│   └── client.yaml          # Client config mẫu
│
├── web/                     # Dashboard UI (Phase 3)
│   ├── index.html
│   ├── style.css
│   └── app.js
│
├── scripts/
│   ├── build.sh             # Cross-compile script
│   └── install.sh           # One-line install script
│
├── Makefile
└── Dockerfile
```

---

## 4. Protocol Design (Giao Thức Giữa Client ↔ Server)

### 4.1 Message Frame Format

Mỗi message truyền qua tunnel sẽ có dạng binary frame:

```
┌──────────┬──────────┬───────────┬──────────────────┐
│ Version  │  Type    │  Length   │     Payload      │
│ (1 byte) │ (1 byte) │ (4 bytes) │  (N bytes)       │
└──────────┴──────────┴───────────┴──────────────────┘
```

### 4.2 Message Types

| Type (hex) | Tên | Mô tả |
|---|---|---|
| `0x01` | `AUTH_REQ` | Client gửi token + client_id lên server |
| `0x02` | `AUTH_RES` | Server phản hồi OK/FAIL |
| `0x03` | `NEW_CONN` | Server báo client: "có request mới, mở proxy stream" |
| `0x04` | `DATA` | Truyền raw bytes (request/response body) |
| `0x05` | `PING` | Heartbeat từ client |
| `0x06` | `PONG` | Server reply heartbeat |
| `0x07` | `CLOSE` | Đóng 1 stream/connection cụ thể |
| `0x08` | `ERROR` | Báo lỗi |

### 4.3 Handshake Flow

```
Client                          Server
  │                                │
  │──── TCP Connect ──────────────▶│  :7000
  │                                │
  │──── AUTH_REQ {token, id} ────▶│
  │                                │  Validate token
  │◀─── AUTH_RES {ok, public_url} ─│
  │                                │
  │◀─── PING ─────────────────────│  (periodic)
  │──── PONG ─────────────────────▶│
  │                                │
  │    ... tunnel ready ...        │
```

---

## 5. Phân Chia Phase & Tasks

---

### Phase 1 — TCP Tunnel Cơ Bản (1 Client, 1 Port)

> **Mục tiêu:** Internet → VPS:8080 → Localhost:3000 hoạt động được.

| # | Task | File liên quan |
|---|---|---|
| 1.1 | Khởi tạo Cargo workspace + 3 crates (`zobite-tunnel-server`, `zobite-tunnel-client`, `zobite-tunnel-protocol`) | `Cargo.toml`, `crates/*/Cargo.toml` |
| 1.2 | Định nghĩa message frame format (Version, Type, Length, Payload) — dùng `bytes` crate | `crates/zobite-tunnel-protocol/src/lib.rs` |
| 1.3 | Viết async reader/writer cho protocol — dùng `tokio::io::{AsyncReadExt, AsyncWriteExt}` | `crates/zobite-tunnel-protocol/src/lib.rs` |
| 1.4 | **Server**: Lắng nghe TCP port 7000 (Control Channel) với `TcpListener`, chờ Client connect | `crates/zobite-tunnel-server/src/server.rs` |
| 1.5 | **Client**: Connect TCP tới `vps:7000` với `TcpStream`, gửi AUTH_REQ đơn giản (hardcode token) | `crates/zobite-tunnel-client/src/client.rs` |
| 1.6 | **Server**: Lắng nghe TCP port 8080 (Public Port). Khi có connection mới → gửi `NEW_CONN` cho Client | `crates/zobite-tunnel-server/src/tunnel.rs` |
| 1.7 | **Client**: Nhận `NEW_CONN` → mở TCP connection tới `localhost:3000` → `tokio::io::copy_bidirectional` | `crates/zobite-tunnel-client/src/proxy.rs` |
| 1.8 | **Pipe bidirectional**: Server pipe bytes giữa public connection ↔ tunnel stream | `crates/zobite-tunnel-server/src/proxy.rs` |
| 1.9 | Implement PING/PONG heartbeat (mỗi 10s) — dùng `tokio::time::interval` | `crates/zobite-tunnel-protocol/src/lib.rs`, `crates/zobite-tunnel-client/src/client.rs` |
| 1.10 | Viết `main.rs` cho cả server và client (CLI flags với `clap` derive) | `crates/*/src/main.rs` |
| 1.11 | **Test**: Chạy 1 HTTP server local → connect client → curl từ ngoài vào VPS:8080 | — |

**Deliverable Phase 1:**
```bash
# Trên VPS
./zobite-tunnel-server --control-port 7000 --public-port 8080

# Trên máy local  
./zobite-tunnel-client --server vps-ip:7000 --local localhost:3000 --token secret123

# Test: trên bất kỳ máy nào
curl http://vps-ip:8080    # → thấy response từ localhost:3000
```

---

### Phase 2 — HTTP Reverse Proxy + Multi-Client

> **Mục tiêu:** Nhiều Client cùng connect. Routing bằng path hoặc subdomain.

| # | Task | File liên quan |
|---|---|---|
| 2.1 | **Client Registry**: `DashMap<String, TunnelConnection>`. Khi Client AUTH thành công thì đăng ký vào registry | `crates/zobite-tunnel-server/src/registry.rs` |
| 2.2 | **HTTP Listener**: Server chuyển public port sang HTTP mode — dùng `hyper`. Parse `Host` header hoặc URL path để xác định client_id | `crates/zobite-tunnel-server/src/proxy.rs` |
| 2.3 | **Path-based routing**: `http://vps-ip/client_a/...` → route tới client_a | `crates/zobite-tunnel-server/src/proxy.rs` |
| 2.4 | **Subdomain routing** (optional): `http://client_a.domain.com` → route tới client_a. Cần wildcard DNS `*.domain.com → VPS IP` | `crates/zobite-tunnel-server/src/proxy.rs` |
| 2.5 | Client config: cho phép user đặt `--id my-tunnel-name` | `crates/zobite-tunnel-client/src/main.rs` |
| 2.6 | **Graceful disconnect**: Khi client ngắt kết nối → xoá khỏi registry → trả 502 cho user | `crates/zobite-tunnel-server/src/registry.rs` |
| 2.7 | **Auto-reconnect**: Client tự reconnect khi mất kết nối (exponential backoff 1s → 2s → 4s → max 30s) | `crates/zobite-tunnel-client/src/client.rs` |
| 2.8 | YAML config file cho cả server và client — dùng `serde` + `serde_yaml` | `configs/server.yaml`, `configs/client.yaml` |

**Deliverable Phase 2:**
```bash
# Client A
./zobite-tunnel-client --server vps:7000 --id webapp --local localhost:3000

# Client B  
./zobite-tunnel-client --server vps:7000 --id api --local localhost:8000

# Truy cập
curl http://vps-ip/webapp/    # → localhost:3000 của máy A
curl http://vps-ip/api/       # → localhost:8000 của máy B
```

---

### Phase 3 — Multiplexing, Auth, Dashboard, HTTPS

> **Mục tiêu:** Production-ready. Nhiều request đồng thời, bảo mật, có dashboard.

| # | Task | File liên quan |
|---|---|---|
| 3.1 | **TCP Multiplexing**: Tích hợp `yamux` crate — 1 TCP connection thật chứa N virtual streams | `crates/zobite-tunnel-protocol/src/lib.rs` (hoặc module `mux`) |
| 3.2 | **Token Auth**: Server có danh sách token hợp lệ, client phải gửi đúng token mới được register | `crates/zobite-tunnel-server/src/server.rs` |
| 3.3 | **Rate Limiting**: Giới hạn số request/s, số connections per client — dùng `governor` crate | `crates/zobite-tunnel-server/src/server.rs` |
| 3.4 | **Metrics Collection**: Đếm bytes in/out, số active connections, request count, latency — dùng `metrics` + `metrics-exporter-prometheus` | `crates/zobite-tunnel-server/src/metrics.rs` |
| 3.5 | **Dashboard API**: REST API `/api/stats`, `/api/clients` — dùng `axum` | `crates/zobite-tunnel-server/src/dashboard.rs` |
| 3.6 | **Dashboard UI**: Web UI đơn giản hiện danh sách clients, traffic chart, logs | `web/*` |
| 3.7 | **TLS/HTTPS**: Hỗ trợ `--tls-cert` và `--tls-key` — dùng `tokio-rustls`, hoặc auto Let's Encrypt với `rustls-acme` | `crates/zobite-tunnel-server/src/server.rs` |
| 3.8 | **Access Log**: Ghi log mỗi request — dùng `tracing` + `tracing-subscriber` | `crates/zobite-tunnel-server/src/server.rs` |
| 3.9 | **TCP mode** (không chỉ HTTP): Cho phép forward raw TCP (ví dụ SSH, database) | `crates/zobite-tunnel-server/src/tunnel.rs` |

---

### Phase 4 — Đóng Gói & Phát Hành (Packaging & Release)

| # | Task | File liên quan |
|---|---|---|
| 4.1 | Makefile: `make build-server`, `make build-client`, `make build-all` | `Makefile` |
| 4.2 | Cross-compile: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` — dùng `cross` hoặc `cargo-zigbuild` | `scripts/build.sh` |
| 4.3 | Dockerfile cho server (multi-stage: `rust:slim` → `debian:bookworm-slim` hoặc `scratch`) | `Dockerfile` |
| 4.4 | Docker Compose (server + dashboard) | `docker-compose.yaml` |
| 4.5 | Install script: `curl -sSL https://... \| bash` | `scripts/install.sh` |
| 4.6 | README.md hoàn chỉnh với hướng dẫn cài đặt, cấu hình, sử dụng | `README.md` |
| 4.7 | GitHub Actions CI/CD: test + build + release binary | `.github/workflows/release.yml` |

---

## 6. Config Files Mẫu

### server.yaml
```yaml
control_port: 7000        # Port cho client kết nối
public_port: 80           # Port cho user truy cập
dashboard_port: 9000      # Port dashboard (optional)
routing_mode: "path"      # "path" hoặc "subdomain"
domain: ""                # Domain nếu dùng subdomain mode
tls:
  enabled: false
  cert: ""
  key: ""
auth:
  tokens:
    - "token_abc123"
    - "token_xyz789"
log_level: "info"
```

### client.yaml
```yaml
server: "vps-ip:7000"
client_id: "my-webapp"
local_addr: "localhost:3000"
token: "token_abc123"
reconnect:
  enabled: true
  max_interval: 30  # seconds
```

---

## 7. Rust Crates Cần Dùng

| Crate | Mục đích |
|---|---|
| `tokio` | Async runtime — TCP listener, stream, timer, task spawning |
| `bytes` | Efficient byte buffer (`BytesMut`, `Buf`, `BufMut`) |
| `hyper` | HTTP/1.1 & HTTP/2 — dùng cho reverse proxy (Phase 2+) |
| `axum` | Web framework cho Dashboard API (Phase 3) |
| `yamux` | TCP multiplexing — nhiều virtual stream trên 1 TCP connection |
| `clap` (derive) | CLI argument parser |
| `serde` + `serde_yaml` | Config file parsing (YAML) |
| `tracing` + `tracing-subscriber` | Structured async logging |
| `tokio-rustls` | TLS support cho public listener |
| `rustls-acme` | Auto Let's Encrypt (Phase 3) |
| `dashmap` | Concurrent hashmap cho client registry |
| `governor` | Rate limiting (Phase 3) |
| `metrics` + `metrics-exporter-prometheus` | Metrics collection (Phase 3) |
| `anyhow` / `thiserror` | Error handling |

---

## 8. Milestone & Ước Tính Thời Gian

| Phase | Mô tả | Ước tính |
|---|---|---|
| **Phase 1** | TCP tunnel cơ bản (1 client, 1 port) | 2-3 ngày |
| **Phase 2** | HTTP proxy + multi-client + routing | 2-3 ngày |
| **Phase 3** | Mux, auth, dashboard, HTTPS | 3-5 ngày |
| **Phase 4** | Đóng gói, Docker, CI/CD | 1-2 ngày |
| **Tổng** | | **~8-13 ngày** |

---

## 9. Tham Khảo (References)

Các dự án tương tự để học hỏi:
- [rathole](https://github.com/rapiz1/rathole) — **Rust**, nhẹ và nhanh, kiến trúc tương tự
- [bore](https://github.com/ekzhang/bore) — **Rust**, cực đơn giản, tham khảo code tốt
- [ngrok](https://github.com/inconshreveable/ngrok) — bản gốc (v1 open-source)
- [frp](https://github.com/fatedier/frp) — Go, rất phổ biến, hỗ trợ TCP/UDP/HTTP
- [localtunnel](https://github.com/localtunnel/localtunnel) — Node.js
- [pgrok](https://github.com/pgrok/pgrok) — Go, self-hosted ngrok alternative

---

> **Bước tiếp theo:** Bắt đầu Phase 1 — Init Cargo workspace và viết protocol message format.
