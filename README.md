# 🚀 Zo Tunnel

**Self-hosted ngrok alternative — expose any local service to the internet through your own VPS.**

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

```
Internet  ───▶  VPS (zo-tunnel-server)  ◀───tunnel───  Your Machine (zo-tunnel-client)  ───▶  localhost:3000
```

---

## ✨ Features

- 🔒 **Token-based authentication** — configurable list of valid tokens
- 🔀 **Multi-client support** — unlimited tunnel clients with path or subdomain routing
- ⚡ **Yamux multiplexing** — multiple streams over a single TCP connection
- 🌐 **HTTP reverse proxy** — intelligent path/subdomain routing with hyper
- 🔌 **Raw TCP tunnels** — dedicated port per client for SSH, databases, any TCP protocol
- 📊 **Live dashboard** — real-time web UI showing clients, metrics, traffic
- 🔐 **TLS/HTTPS support** — optional TLS for the public listener
- 🛡️ **Rate limiting** — per-client request throttling
- 📈 **Metrics** — bytes in/out, request counts, active connections per client
- 🔄 **Auto-reconnect** — exponential backoff (1s → 2s → 4s → ... → 30s)
- 💓 **Yamux keep-alive** — built-in connection health monitoring
- 📦 **Single static binary** — ~1.8MB client, ~5.5MB server (release, stripped)
- 🐳 **Docker ready** — multi-stage Dockerfile + Docker Compose
- 🔧 **YAML config** — file-based configuration with CLI override support

---

## 📐 Architecture

```
┌─────────────────┐          ┌──────────────────────────┐          ┌─────────────────┐
│  Public User     │  HTTP    │     zo-tunnel-server (VPS)   │  Tunnel  │  zo-tunnel-client   │
│  (Browser/curl)  │────────▶│                          │◀─────────│  (Local Machine) │
│                  │◀────────│  :6210 public HTTP proxy │─────────▶│                  │
└─────────────────┘  Response│  :6200 control channel   │  yamux   │  localhost:3000  │
                             │  :6220 dashboard         │  mux/TCP │                  │
                             └──────────────────────────┘          └─────────────────┘
```

### Data Flow

```
1. Client  ──TCP──▶  Server:6200     (Connect + AUTH handshake)
2. Both sides upgrade to yamux multiplexed session
3. User    ──HTTP──▶ Server:6210     (Public request: /client-id/path)
4. Server extracts routing, opens yamux stream to target client
5. hyper HTTP client sends request through yamux stream
6. Client accepts yamux stream → pipes to localhost:3000
7. Response flows back: local → client → yamux → server → user
```

---

## 🚀 Quick Start

### 1. Install server on VPS (Linux)

```bash
curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/setup-server.sh | sudo bash
```

Automated script: download binary → create systemd service → open firewall → start server.
Once completed, it will display the **token** and the **client connect command**.

Or install with a custom token:

```bash
curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/setup-server.sh | ZO_TOKEN=my-secret sudo bash
```

### 2. Install client on local machine (macOS / Linux)

```bash
curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | bash
```

### 3. Connect

```bash
zo-tunnel-client \
  --server your-vps-ip:6200 \
  --local localhost:3000 \
  --id my-webapp \
  --token my-secret-token
```

### 4. Access from anywhere

```bash
curl http://your-vps-ip:6210/my-webapp/
# → Response from your localhost:3000 🎉

# Dashboard
open http://your-vps-ip:6220
```

### Multi-Client Example

```bash
# Client A — web frontend (HTTP mode)
# Client A — web frontend (HTTP mode)
zo-tunnel-client --server vps:6200 --id webapp --local localhost:3000 --token secret

# Client B — API server (HTTP mode)
zo-tunnel-client --server vps:6200 --id api --local localhost:8000 --token secret

# Client C — SSH server (TCP mode — gets dedicated port)
zo-tunnel-client --server vps:6200 --id ssh --local localhost:22 --token secret --tcp

# Access HTTP tunnels
curl http://vps:6210/webapp/     # → localhost:3000 (Client A)
curl http://vps:6210/api/users   # → localhost:8000/users (Client B)

# Access TCP tunnel (port assigned by server, e.g. 10000)
ssh user@vps -p 10000            # → localhost:22 (Client C)
```

### Build from source (optional)

```bash
git clone https://github.com/Zobite/zo-tunnel.git
cd zo-tunnel
cargo build --release
# → target/release/zo-tunnel-server (5.5 MB)
# → target/release/zo-tunnel-client (1.8 MB)
```

---

## 📖 CLI Reference

### `zo-tunnel-server`

| Flag | Default | Env Var | Description |
|---|---|---|---|
| `--config`, `-c` | — | `ZO_CONFIG` | Path to YAML config file |
| `--control-port` | `6200` | `ZO_CONTROL_PORT` | Client control channel port |
| `--public-port` | `6210` | `ZO_PUBLIC_PORT` | Public HTTP proxy port |
| `--dashboard-port` | `6220` | `ZO_DASHBOARD_PORT` | Dashboard UI port |
| `--token` | — | `ZO_TOKEN` | Auth token(s), comma-separated |
| `--routing-mode` | `path` | `ZO_ROUTING_MODE` | `path` or `subdomain` |
| `--domain` | — | `ZO_DOMAIN` | Domain for subdomain routing |
| `--tls-cert` | — | `ZO_TLS_CERT` | TLS certificate file (PEM) |
| `--tls-key` | — | `ZO_TLS_KEY` | TLS private key file (PEM) |

### `zo-tunnel-client`

| Flag | Default | Env Var | Description |
|---|---|---|---|
| `--config`, `-c` | — | `ZO_CONFIG` | Path to YAML config file |
| `--server` | — | `ZO_SERVER` | Server address (host:port) |
| `--local` | `localhost:3000` | `ZO_LOCAL` | Local service to forward to |
| `--id` | `default` | `ZO_CLIENT_ID` | Tunnel name (used for routing) |
| `--token` | — | `ZO_TOKEN` | Auth token |
| `--tcp` | `false` | — | Request dedicated TCP port (raw TCP mode) |
| `--no-reconnect` | `false` | — | Disable auto-reconnect |

---

## ⚙️ Configuration

Both server and client support YAML config files (see `configs/` for examples):

```yaml
# configs/server.yaml
control_port: 6200
public_port: 80
dashboard_port: 6220
routing_mode: "path"
auth:
  tokens:
    - "token_abc123"
rate_limit:
  requests_per_second: 100
tcp_ports:
  enabled: true
  port_start: 10000
  port_end: 10100
tls:
  enabled: false
  cert: "/path/to/cert.pem"
  key: "/path/to/key.pem"
```

```yaml
# configs/client.yaml
server: "vps-ip:6200"
client_id: "my-webapp"
local_addr: "localhost:3000"
token: "token_abc123"
tcp_mode: false              # Set true for SSH/database/raw TCP
reconnect:
  enabled: true
  max_interval: 30
```

CLI flags override config file values.

---

## 🔌 Protocol

### Handshake (pre-yamux)

```
Client                             Server
  │                                   │
  │──── TCP Connect ─────────────────▶│  :6200
  │──── AUTH_REQ {client_id, token} ─▶│
  │◀─── AUTH_RES {ok, public_port} ──│  validate token
  │                                   │
  │ ═══ Upgrade to yamux session ══════│
  │                                   │
  │  ... multiplexed tunnel ready ... │
```

### Binary Frame Format (auth messages)

```
┌──────────┬──────────┬───────────┬──────────────────┐
│ Version  │  Type    │  Length   │     Payload      │
│ (1 byte) │ (1 byte) │ (4 bytes) │  (N bytes)       │
└──────────┴──────────┴───────────┴──────────────────┘
```

### Request Flow (after yamux)

```
1. Public user → HTTP request → Server:6210
2. Server: parse Host/path → determine client_id
3. Server: open yamux stream to client
4. Server: hyper HTTP client → sends request through yamux stream
5. Client: accepts yamux stream → connects to localhost → pipes bidirectionally
6. Response flows back through yamux → hyper → public user
```

---

## 📊 Dashboard

The server includes a built-in web dashboard at the dashboard port (default: 6220):

- **Server status** — uptime, version
- **Connected clients** — list with connection duration
- **Live metrics** — requests, bytes transferred, active connections
- **Rate limit stats** — failed auth attempts, throttled requests

Auto-refreshes every 2 seconds.

### Dashboard API

| Endpoint | Description |
|---|---|
| `GET /api/status` | Server status + version |
| `GET /api/clients` | List of connected tunnel clients |
| `GET /api/metrics` | Global traffic metrics |

---

## 📁 Project Structure

```
zo-tunnel/
├── Cargo.toml                    # Workspace (3 crates)
├── PLAN.md                       # Implementation plan
├── README.md                     # This file
├── Makefile                      # Build, test, Docker commands
├── Dockerfile                    # Multi-stage (server + client targets)
├── docker-compose.yaml           # Server deployment
│
├── crates/
│   ├── zo-tunnel-protocol/           # Shared protocol library
│   │   └── src/lib.rs            #   Message types, frame encoding, constants
│   │
│   ├── zo-tunnel-server/             # Server binary
│   │   └── src/
│   │       ├── main.rs           #   CLI + config loading
│   │       ├── config.rs         #   YAML config with all options
│   │       ├── server.rs         #   Core: control channel, yamux driver, HTTP proxy
│   │       ├── registry.rs       #   Client registry (DashMap)
│   │       ├── proxy.rs          #   HTTP reverse proxy with routing
│   │       ├── dashboard.rs      #   Dashboard REST API (axum) + embedded UI
│   │       └── metrics.rs        #   Metrics + rate limiter
│   │
│   └── zo-tunnel-client/             # Client binary
│       └── src/
│           ├── main.rs           #   CLI + exponential backoff reconnect
│           ├── config.rs         #   YAML config
│           └── client.rs         #   Auth, yamux session, stream proxy
│
├── configs/                      # Sample YAML configs
├── web/                          # Dashboard UI (HTML/CSS/JS)
├── scripts/                      # build.sh, install.sh, e2e_test.sh
└── .github/workflows/ci.yml     # CI/CD pipeline
```

---

## 🧰 Tech Stack

| Crate | Purpose |
|---|---|
| [`tokio`](https://tokio.rs/) | Async runtime |
| [`yamux`](https://docs.rs/yamux) | TCP multiplexing (multiple streams per connection) |
| [`hyper`](https://hyper.rs/) | HTTP/1.1 reverse proxy (server + client) |
| [`axum`](https://docs.rs/axum) | Dashboard REST API framework |
| [`tokio-rustls`](https://docs.rs/tokio-rustls) | TLS support |
| [`clap`](https://docs.rs/clap) | CLI parsing with env var support |
| [`serde`](https://serde.rs/) + `serde_yaml` | Config + message serialization |
| [`dashmap`](https://docs.rs/dashmap) | Concurrent client registry |
| [`tracing`](https://docs.rs/tracing) | Structured async logging |
| [`tower-http`](https://docs.rs/tower-http) | HTTP middleware |

---

## 🐳 Docker

### Quick Start (no source code needed)

Run the server directly from the pre-built image — no cloning required:

```bash
docker run -d \
  --name zo-tunnel-server \
  -p 6200:6200 \
  -p 6210:6210 \
  -p 6220:6220 \
  -p 10000-10020:10000-10020 \
  -e ZO_TOKEN=my-secret-token \
  -e RUST_LOG=info \
  --restart unless-stopped \
  ghcr.io/zobite/zo-tunnel-server:latest
```

That's it! Your server is now running. Connect a client:

```bash
zo-tunnel-client --server your-vps-ip:6200 --local localhost:3000 --id myapp --token my-secret-token
```

| Port | Purpose |
|---|---|
| `6200` | Control channel (client connections) |
| `6210` | Public HTTP proxy |
| `6220` | Dashboard UI |
| `10000-10020` | Dedicated TCP tunnel ports |

### With a custom config file

```bash
docker run -d \
  --name zo-tunnel-server \
  -p 6200:6200 -p 6210:6210 -p 6220:6220 \
  -p 10000-10020:10000-10020 \
  -v /path/to/server.yaml:/etc/zo-tunnel/server.yaml \
  --restart unless-stopped \
  ghcr.io/zobite/zo-tunnel-server:latest \
  --config /etc/zo-tunnel/server.yaml
```

### Docker Compose (requires cloning the repo)

```bash
git clone https://github.com/Zobite/zo-tunnel.git && cd zo-tunnel

ZO_TOKEN=my-secret-token docker compose up -d

# Check logs
docker compose logs -f
```

### Build images from source

```bash
git clone https://github.com/Zobite/zo-tunnel.git && cd zo-tunnel

# Build server image
docker build -t zo-tunnel-server --target server .

# Build client image
docker build -t zo-tunnel-client --target client .
```

---

## 🧪 Testing

```bash
# Unit tests
cargo test --workspace

# E2E integration test
cargo build --release
bash scripts/e2e_test.sh
```

---

## 🗺️ Roadmap

| Feature | Status |
|---|---|
| TCP tunnel (single client) | ✅ Done |
| Binary protocol + auth | ✅ Done |
| Yamux multiplexing | ✅ Done |
| Multi-client support | ✅ Done |
| HTTP reverse proxy | ✅ Done |
| Path-based routing | ✅ Done |
| Subdomain routing | ✅ Done |
| **Dedicated TCP tunnels** | ✅ Done |
| Dashboard API + UI | ✅ Done |
| Rate limiting | ✅ Done |
| Metrics collection | ✅ Done |
| TLS/HTTPS | ✅ Done |
| YAML config files | ✅ Done |
| Auto-reconnect (exp backoff) | ✅ Done |
| Dockerfile + Compose | ✅ Done |
| CI/CD (GitHub Actions) | ✅ Done |
| Cross-compile scripts | ✅ Done |
| Install script | ✅ Done |

---

## 📄 License

MIT — see [LICENSE](LICENSE) for details.

---

**Built with ❤️ and 🦀 Rust**
