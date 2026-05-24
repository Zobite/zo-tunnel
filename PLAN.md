# Zobite Zo Tunnel — Implementation Plan

> **Self-hosted ngrok alternative.**
> Expose any local service to the internet through your own VPS.

---

## 1. Overview

| Item | Details |
|---|---|
| **Project Name** | Zobite Zo Tunnel |
| **Goal** | Allow internet users to access services running on a local machine (behind NAT/Firewall) through a VPS intermediary |
| **Components** | `zo-tunnel-server` (runs on VPS) + `zo-tunnel-client` (runs on local machine) |
| **Language** | **Rust** — static binary, zero-cost abstractions, memory safety, performance on par with C/C++, powerful async I/O with Tokio |
| **License** | MIT (or configurable) |

---

## 2. Architecture

```
┌─────────────────┐          ┌──────────────────────────┐          ┌─────────────────┐
│  Public User     │  HTTP    │     zo-tunnel-server (VPS)   │  Tunnel  │  zo-tunnel-client   │
│  (Browser/curl)  │────────▶│                          │◀─────────│  (Local Machine) │
│                  │◀────────│  :80 public listener     │─────────▶│                  │
└─────────────────┘  Response│  :6200 control channel   │  Mux/TCP │  localhost:3000  │
                             │  :6220 dashboard (opt)   │          └─────────────────┘
                             └──────────────────────────┘
```

### Data Flow

```
1. Client ──TCP/WebSocket──▶ Server:6200   (Register + keep-alive)
2. User   ──HTTP──────────▶ Server:80      (Public request)
3. Server ──Tunnel─────────▶ Client        (Forward request bytes)
4. Client ──HTTP───────────▶ localhost:X    (Proxy to local service)
5. Response flows back: Local → Client → Server → User
```

---

## 3. Planned Project Structure

```
zobite_zo-tunnel/
├── PLAN.md                  # This file
├── README.md
├── Cargo.toml               # Workspace root
├── Cargo.lock
│
├── crates/
│   ├── zo-tunnel-server/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs          # Entry point for zo-tunnel-server
│   │       ├── server.rs        # Core server logic
│   │       ├── tunnel.rs        # Tunnel connection management
│   │       ├── proxy.rs         # HTTP reverse proxy handler
│   │       ├── registry.rs      # Client registry (map client_id → connection)
│   │       ├── dashboard.rs     # Dashboard API (Phase 3)
│   │       └── metrics.rs       # Metrics collection (Phase 3)
│   │
│   ├── zo-tunnel-client/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs          # Entry point for zo-tunnel-client
│   │       ├── client.rs        # Core client logic
│   │       ├── tunnel.rs        # Tunnel connection handler
│   │       └── proxy.rs         # Local proxy (forward to localhost:X)
│   │
│   └── zo-tunnel-protocol/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs           # Message frame format, reader/writer, shared types
│
├── configs/
│   ├── server.yaml          # Sample server config
│   └── client.yaml          # Sample client config
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

## 4. Protocol Design (Client ↔ Server)

### 4.1 Message Frame Format

Each message transmitted through the tunnel uses a binary frame format:

```
┌──────────┬──────────┬───────────┬──────────────────┐
│ Version  │  Type    │  Length   │     Payload      │
│ (1 byte) │ (1 byte) │ (4 bytes) │  (N bytes)       │
└──────────┴──────────┴───────────┴──────────────────┘
```

### 4.2 Message Types

| Type (hex) | Name | Description |
|---|---|---|
| `0x01` | `AUTH_REQ` | Client sends token + client_id to server |
| `0x02` | `AUTH_RES` | Server responds OK/FAIL |
| `0x03` | `NEW_CONN` | Server notifies client: "new request, open proxy stream" |
| `0x04` | `DATA` | Transmit raw bytes (request/response body) |
| `0x05` | `PING` | Heartbeat from client |
| `0x06` | `PONG` | Server heartbeat reply |
| `0x07` | `CLOSE` | Close a specific stream/connection |
| `0x08` | `ERROR` | Error notification |

### 4.3 Handshake Flow

```
Client                          Server
  │                                │
  │──── TCP Connect ──────────────▶│  :6200
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

## 5. Phase Breakdown & Tasks

---

### Phase 1 — Basic TCP Tunnel (1 Client, 1 Port)

> **Goal:** Internet → VPS:6210 → Localhost:3000 working end-to-end.

| # | Task | Related Files |
|---|---|---|
| 1.1 | Initialize Cargo workspace + 3 crates (`zo-tunnel-server`, `zo-tunnel-client`, `zo-tunnel-protocol`) | `Cargo.toml`, `crates/*/Cargo.toml` |
| 1.2 | Define message frame format (Version, Type, Length, Payload) — using `bytes` crate | `crates/zo-tunnel-protocol/src/lib.rs` |
| 1.3 | Write async reader/writer for protocol — using `tokio::io::{AsyncReadExt, AsyncWriteExt}` | `crates/zo-tunnel-protocol/src/lib.rs` |
| 1.4 | **Server**: Listen on TCP port 6200 (Control Channel) with `TcpListener`, await Client connections | `crates/zo-tunnel-server/src/server.rs` |
| 1.5 | **Client**: Connect TCP to `vps:6200` with `TcpStream`, send simple AUTH_REQ (hardcoded token) | `crates/zo-tunnel-client/src/client.rs` |
| 1.6 | **Server**: Listen on TCP port 6210 (Public Port). On new connection → send `NEW_CONN` to Client | `crates/zo-tunnel-server/src/tunnel.rs` |
| 1.7 | **Client**: Receive `NEW_CONN` → open TCP connection to `localhost:3000` → `tokio::io::copy_bidirectional` | `crates/zo-tunnel-client/src/proxy.rs` |
| 1.8 | **Pipe bidirectional**: Server pipes bytes between public connection ↔ tunnel stream | `crates/zo-tunnel-server/src/proxy.rs` |
| 1.9 | Implement PING/PONG heartbeat (every 10s) — using `tokio::time::interval` | `crates/zo-tunnel-protocol/src/lib.rs`, `crates/zo-tunnel-client/src/client.rs` |
| 1.10 | Write `main.rs` for both server and client (CLI flags with `clap` derive) | `crates/*/src/main.rs` |
| 1.11 | **Test**: Run a local HTTP server → connect client → curl from outside into VPS:6210 | — |

**Deliverable Phase 1:**
```bash
# On the VPS
./zo-tunnel-server --control-port 6200 --public-port 6210

# On the local machine
./zo-tunnel-client --server vps-ip:6200 --local localhost:3000 --token secret123

# Test: from any machine
curl http://vps-ip:6210    # → see response from localhost:3000
```

---

### Phase 2 — HTTP Reverse Proxy + Multi-Client

> **Goal:** Multiple clients connected simultaneously. Routing by path or subdomain.

| # | Task | Related Files |
|---|---|---|
| 2.1 | **Client Registry**: `DashMap<String, TunnelConnection>`. When Client AUTH succeeds, register in registry | `crates/zo-tunnel-server/src/registry.rs` |
| 2.2 | **HTTP Listener**: Server switches public port to HTTP mode — using `hyper`. Parse `Host` header or URL path to determine client_id | `crates/zo-tunnel-server/src/proxy.rs` |
| 2.3 | **Path-based routing**: `http://vps-ip/client_a/...` → route to client_a | `crates/zo-tunnel-server/src/proxy.rs` |
| 2.4 | **Subdomain routing** (optional): `http://client_a.domain.com` → route to client_a. Requires wildcard DNS `*.domain.com → VPS IP` | `crates/zo-tunnel-server/src/proxy.rs` |
| 2.5 | Client config: allow user to set `--id my-tunnel-name` | `crates/zo-tunnel-client/src/main.rs` |
| 2.6 | **Graceful disconnect**: When client disconnects → remove from registry → return 502 to user | `crates/zo-tunnel-server/src/registry.rs` |
| 2.7 | **Auto-reconnect**: Client auto-reconnects on connection loss (exponential backoff 1s → 2s → 4s → max 30s) | `crates/zo-tunnel-client/src/client.rs` |
| 2.8 | YAML config file for both server and client — using `serde` + `serde_yaml` | `configs/server.yaml`, `configs/client.yaml` |

**Deliverable Phase 2:**
```bash
# Client A
./zo-tunnel-client --server vps:6200 --id webapp --local localhost:3000

# Client B
./zo-tunnel-client --server vps:6200 --id api --local localhost:8000

# Access
curl http://vps-ip/webapp/    # → localhost:3000 on machine A
curl http://vps-ip/api/       # → localhost:8000 on machine B
```

---

### Phase 3 — Multiplexing, Auth, Dashboard, HTTPS

> **Goal:** Production-ready. Concurrent requests, security, dashboard.

| # | Task | Related Files |
|---|---|---|
| 3.1 | **TCP Multiplexing**: Integrate `yamux` crate — 1 real TCP connection contains N virtual streams | `crates/zo-tunnel-protocol/src/lib.rs` (or `mux` module) |
| 3.2 | **Token Auth**: Server maintains a list of valid tokens, client must send correct token to register | `crates/zo-tunnel-server/src/server.rs` |
| 3.3 | **Rate Limiting**: Limit requests/s, connections per client — using `governor` crate | `crates/zo-tunnel-server/src/server.rs` |
| 3.4 | **Metrics Collection**: Count bytes in/out, active connections, request count, latency — using `metrics` + `metrics-exporter-prometheus` | `crates/zo-tunnel-server/src/metrics.rs` |
| 3.5 | **Dashboard API**: REST API `/api/stats`, `/api/clients` — using `axum` | `crates/zo-tunnel-server/src/dashboard.rs` |
| 3.6 | **Dashboard UI**: Simple web UI showing client list, traffic chart, logs | `web/*` |
| 3.7 | **TLS/HTTPS**: Support `--tls-cert` and `--tls-key` — using `tokio-rustls`, or auto Let's Encrypt with `rustls-acme` | `crates/zo-tunnel-server/src/server.rs` |
| 3.8 | **Access Log**: Log each request — using `tracing` + `tracing-subscriber` | `crates/zo-tunnel-server/src/server.rs` |
| 3.9 | **TCP mode** (not just HTTP): Allow forwarding raw TCP (e.g. SSH, database) | `crates/zo-tunnel-server/src/tunnel.rs` |

---

### Phase 4 — Packaging & Release

| # | Task | Related Files |
|---|---|---|
| 4.1 | Makefile: `make build-server`, `make build-client`, `make build-all` | `Makefile` |
| 4.2 | Cross-compile: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` — using `cross` or `cargo-zigbuild` | `scripts/build.sh` |
| 4.3 | Dockerfile for server (multi-stage: `rust:slim` → `debian:bookworm-slim` or `scratch`) | `Dockerfile` |
| 4.4 | Docker Compose (server + dashboard) | `docker-compose.yaml` |
| 4.5 | Install script: `curl -sSL https://... \| bash` | `scripts/install.sh` |
| 4.6 | Complete README.md with installation, configuration, and usage guides | `README.md` |
| 4.7 | GitHub Actions CI/CD: test + build + release binary | `.github/workflows/release.yml` |

---

## 6. Sample Config Files

### server.yaml
```yaml
control_port: 6200        # Port for client connections
public_port: 80           # Port for user access
dashboard_port: 6220      # Dashboard port (optional)
routing_mode: "path"      # "path" or "subdomain"
domain: ""                # Domain if using subdomain mode
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
server: "vps-ip:6200"
client_id: "my-webapp"
local_addr: "localhost:3000"
token: "token_abc123"
reconnect:
  enabled: true
  max_interval: 30  # seconds
```

---

## 7. Required Rust Crates

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime — TCP listener, stream, timer, task spawning |
| `bytes` | Efficient byte buffer (`BytesMut`, `Buf`, `BufMut`) |
| `hyper` | HTTP/1.1 & HTTP/2 — used for reverse proxy (Phase 2+) |
| `axum` | Web framework for Dashboard API (Phase 3) |
| `yamux` | TCP multiplexing — multiple virtual streams on 1 TCP connection |
| `clap` (derive) | CLI argument parser |
| `serde` + `serde_yaml` | Config file parsing (YAML) |
| `tracing` + `tracing-subscriber` | Structured async logging |
| `tokio-rustls` | TLS support for public listener |
| `rustls-acme` | Auto Let's Encrypt (Phase 3) |
| `dashmap` | Concurrent hashmap for client registry |
| `governor` | Rate limiting (Phase 3) |
| `metrics` + `metrics-exporter-prometheus` | Metrics collection (Phase 3) |
| `anyhow` / `thiserror` | Error handling |

---

## 8. Milestones & Time Estimates

| Phase | Description | Estimate |
|---|---|---|
| **Phase 1** | Basic TCP tunnel (1 client, 1 port) | 2-3 days |
| **Phase 2** | HTTP proxy + multi-client + routing | 2-3 days |
| **Phase 3** | Mux, auth, dashboard, HTTPS | 3-5 days |
| **Phase 4** | Packaging, Docker, CI/CD | 1-2 days |
| **Total** | | **~8-13 days** |

---

## 9. References

Similar projects for reference:
- [rathole](https://github.com/rapiz1/rathole) — **Rust**, lightweight and fast, similar architecture
- [bore](https://github.com/ekzhang/bore) — **Rust**, extremely simple, good code reference
- [ngrok](https://github.com/inconshreveable/ngrok) — the original (v1 open-source)
- [frp](https://github.com/fatedier/frp) — Go, very popular, supports TCP/UDP/HTTP
- [localtunnel](https://github.com/localtunnel/localtunnel) — Node.js
- [pgrok](https://github.com/pgrok/pgrok) — Go, self-hosted ngrok alternative

---

> **Next step:** Start Phase 1 — Init Cargo workspace and write the protocol message format.
