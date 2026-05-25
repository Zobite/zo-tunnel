# 🚀 Zo Tunnel

**Self-hosted ngrok alternative — expose any local service to the internet through your own VPS.**

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

```mermaid
flowchart LR
    A["🌐 Internet"] -->|HTTP| B["VPS (zo-tunnel-server)"]
    B <-->|yamux tunnel| C["Your Machine (zo-tunnel-client)"]
    C --> D["localhost:3000"]
```

---

## ✨ Features

- 🌐 **Subdomain routing** — `myapp.tunnel.example.com` for each client
- 🔒 **Token-based auth** — configurable list of valid tokens
- ⚡ **Yamux multiplexing** — multiple streams over a single TCP connection
- 📊 **Live dashboard** — real-time web UI at `dashboard.<domain>`
- 🔐 **TLS control channel** — optional TLS for client ↔ server communication
- 🛡️ **Rate limiting** — per-client request throttling
- 🔄 **Auto-reconnect** — exponential backoff (1s → 30s)
- 📦 **Single static binary** — ~3.6MB server, ~2.9MB client
- 🔧 **CLI setup** — one command to configure, one to start

---

## 📐 Architecture

```mermaid
flowchart LR
    subgraph Internet
        User["👤 Public User"]
    end

    subgraph VPS["zo-tunnel-server"]
        Control[":6200 Control Channel"]
        Public[":6210 Public HTTP"]
        Sub["*.tunnel.example.com"]
    end

    subgraph Local["zo-tunnel-client"]
        Client["Your Machine"]
        Service["localhost:3000"]
    end

    User -->|"HTTP Request"| Public
    Client <-->|"yamux (TLS optional)"| Control
    Client --> Service
```

| Port | Role |
|---|---|
| `:6200` | Control channel — client ↔ server yamux (optional TLS) |
| `:6210` | Public HTTP — subdomain routing + dashboard |

Each client is accessible at `<client_id>.<domain>`, dashboard at `dashboard.<domain>`.

---

## 🚀 Quick Start

### 1. Install server on VPS

```bash
# Download pre-built binary
curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | sudo bash -s server

# Setup (domain is required)
zo-tunnel-server setup --domain tunnel.example.com

# Start
zo-tunnel-server start
```

Setup prints your **token** and **client connect command**.

### 2. DNS setup

Add a wildcard A record pointing to your VPS:

```
*.tunnel.example.com  →  YOUR_VPS_IP
```

### 3. Install client on local machine

```bash
curl -sSL https://raw.githubusercontent.com/Zobite/zo-tunnel/main/scripts/install.sh | bash -s client
```

### 4. Connect

```bash
zo-tunnel-client \
  --server YOUR_VPS_IP:6200 \
  --local localhost:3000 \
  --id my-api \
  --token YOUR_TOKEN
```

Access at: `http://my-api.tunnel.example.com` 🎉

### Multi-client example

```bash
# Web frontend
zo-tunnel-client --server vps:6200 --id webapp --local localhost:3000 --token secret
# → http://webapp.tunnel.example.com

# API server
zo-tunnel-client --server vps:6200 --id api --local localhost:8080 --token secret
# → http://api.tunnel.example.com

# Ollama
zo-tunnel-client --server vps:6200 --id ollama --local localhost:11434 --token secret
# → http://ollama.tunnel.example.com
```

---

## 🔒 Traefik + SSL (Recommended)

For production with HTTPS, put [Traefik](https://traefik.io/) in front of zo-tunnel's public port:

```yaml
# traefik dynamic config
http:
  routers:
    zo-tunnel:
      rule: "HostRegexp(`{subdomain:.+}.tunnel.example.com`)"
      service: zo-tunnel
      tls:
        certResolver: letsencrypt
        domains:
          - main: "tunnel.example.com"
            sans: ["*.tunnel.example.com"]
  services:
    zo-tunnel:
      loadBalancer:
        servers:
          - url: "http://127.0.0.1:6210"
```

All tunnels get **HTTPS automatically** via Let's Encrypt wildcard cert. No per-client configuration needed.

---

## 📖 CLI Reference

### Server

#### `zo-tunnel-server setup`

| Flag | Default | Description |
|---|---|---|
| `--domain` | *(required)* | Base domain for subdomain routing |
| `--control-port` | `6200` | Client control channel port |
| `--public-port` | `6210` | Public HTTP port |
| `--token` | *(auto-generated)* | Client auth token |
| `--dashboard-token` | *(auto-generated)* | Dashboard admin token |
| `--tls-cert` | — | TLS certificate file (PEM) |
| `--tls-key` | — | TLS private key file (PEM) |
| `--force` | — | Overwrite existing config |

#### `zo-tunnel-server start`

Loads saved config and starts the server. No additional flags.

#### `zo-tunnel-server status`

Displays current configuration summary and token info.

#### `zo-tunnel-server upgrade`

Self-upgrade to the latest version from GitHub releases.

```bash
zo-tunnel-server upgrade
```

- Checks GitHub for the latest release
- Compares with current version — skips if already up-to-date
- Downloads the correct binary for your OS/arch
- Replaces the binary in `/usr/local/bin/` (uses `sudo` if needed)

> **Note:** Restart the service after upgrading: `sudo systemctl restart zo-tunnel`

#### `zo-tunnel-server uninstall`

Remove the server binary, systemd service, and config.

```bash
zo-tunnel-server uninstall           # interactive confirmation
zo-tunnel-server uninstall --yes     # skip confirmation
zo-tunnel-server uninstall --keep-config  # preserve /etc/zo-tunnel/
```

| Flag | Description |
|---|---|
| `--yes` / `-y` | Skip confirmation prompt |
| `--keep-config` | Keep config files, only remove binary and service |

### Client

#### `zo-tunnel-client [connect]`

Connect to the tunnel server. The `connect` subcommand is optional — you can use the flags directly.

```bash
# Both are equivalent:
zo-tunnel-client --server vps:6200 --local localhost:3000 --id my-app --token SECRET
zo-tunnel-client connect --server vps:6200 --local localhost:3000 --id my-app --token SECRET
```

| Flag | Env var | Default | Description |
|---|---|---|---|
| `--server` | `ZO_SERVER` | — | Server address (`host:port`) |
| `--local` | `ZO_LOCAL` | `localhost:3000` | Local service to forward to |
| `--id` | `ZO_CLIENT_ID` | `default` | Tunnel name (becomes subdomain) |
| `--token` | `ZO_TOKEN` | — | Auth token |
| `--config` / `-c` | `ZO_CONFIG` | — | Path to YAML config file |
| `--no-reconnect` | — | `false` | Disable auto-reconnect |
| `--tls` | `ZO_TLS` | `false` | Enable TLS for control channel |
| `--tls-server-name` | — | *(from --server)* | Server name for TLS SNI |
| `--tls-skip-verify` | — | `false` | Skip TLS cert verification (⚠️ dev only) |

#### `zo-tunnel-client upgrade`

Self-upgrade to the latest version from GitHub releases.

```bash
zo-tunnel-client upgrade
```

#### `zo-tunnel-client uninstall`

Remove the client binary.

```bash
zo-tunnel-client uninstall           # interactive confirmation
zo-tunnel-client uninstall --yes     # skip confirmation
```

---

## ⚙️ Configuration

Config is generated by `zo-tunnel-server setup` and saved to `/etc/zo-tunnel/server.yaml`.

### Server

```yaml
control_port: 6200
public_port: 6210
domain: "tunnel.example.com"

auth:
  tokens:
    - "your-token"

dashboard_auth:
  token: "your-dashboard-token"
  session_ttl_secs: 86400

rate_limit:
  requests_per_second: 100
  max_connections_per_client: 50

# Optional TLS for control channel
tls:
  enabled: false
  cert: "/etc/zo-tunnel/server.crt"
  key: "/etc/zo-tunnel/server.key"

log_level: "info"
```

### Client

```yaml
server: "vps-ip:6200"
client_id: "my-webapp"
local_addr: "localhost:3000"
token: "your-token"

reconnect:
  enabled: true
  max_interval: 30

# tls:
#   enabled: true
#   server_name: ""
#   skip_verify: false
```

---

## 🔌 Protocol

### Handshake

```mermaid
sequenceDiagram
    participant C as Client
    participant S as Server (:6200)

    C->>S: TCP Connect
    C->>S: AUTH_REQ {client_id, token}
    S-->>C: AUTH_RES {ok, route}
    Note over C,S: Upgrade to yamux session
    Note over C,S: 🚇 Multiplexed tunnel ready
```

### Binary frame format

```
┌──────────┬──────────┬───────────┬──────────────────┐
│ Version  │  Type    │  Length   │     Payload      │
│ (1 byte) │ (1 byte) │ (4 bytes) │  (N bytes)       │
└──────────┴──────────┴───────────┴──────────────────┘
```

### Request flow

```mermaid
sequenceDiagram
    participant U as User
    participant S as Server (:6210)
    participant C as Client
    participant L as localhost:3000

    U->>S: HTTP (Host: myapp.tunnel.example.com)
    S->>S: Parse Host → find client "myapp"
    S->>C: Open yamux stream
    C->>L: Proxy request
    L-->>C: Response
    C-->>S: Response via yamux
    S-->>U: HTTP Response
```

---

## 📊 Dashboard

Built-in web dashboard at `dashboard.<domain>`:

- **Server status** — uptime, version
- **Connected clients** — list with connection duration
- **Live metrics** — requests, bytes transferred, active connections
- **Rate limit stats** — failed auth attempts, throttled requests

Auto-refreshes every 2 seconds.

| Endpoint | Description |
|---|---|
| `GET /api/status` | Server status and version |
| `GET /api/clients` | Connected tunnel clients |
| `GET /api/metrics` | Global traffic metrics |

---

## 🔐 TLS

TLS encrypts the control channel (`:6200`) — auth tokens and all multiplexed tunnel data.

```bash
# Server: enable TLS
zo-tunnel-server setup \
  --domain tunnel.example.com \
  --tls-cert /path/to/fullchain.pem \
  --tls-key /path/to/privkey.pem

# Client: connect with TLS
zo-tunnel-client --server tunnel.example.com:6200 \
  --local localhost:3000 --id my-app --token YOUR_TOKEN \
  --tls

# Client: self-signed cert (dev only)
zo-tunnel-client --server 192.168.1.100:6200 \
  --local localhost:3000 --id my-app --token YOUR_TOKEN \
  --tls --tls-skip-verify
```

| Component | Encryption |
|---|---|
| Control channel (`:6200`) | ✅ TLS (when enabled) |
| Public HTTP (`:6210`) | Use Traefik/nginx for SSL termination |

---

## 📁 Project Structure

```
zo-tunnel/
├── Cargo.toml                    # Workspace (3 crates)
├── Makefile
│
├── crates/
│   ├── zo-tunnel-protocol/       # Shared protocol library
│   │   └── src/
│   │       ├── lib.rs            #   Messages, frame encoding, constants
│   │       └── self_update.rs    #   Self-upgrade + uninstall logic
│   │
│   ├── zo-tunnel-server/         # Server binary
│   │   └── src/
│   │       ├── main.rs           #   CLI: setup / start / status / upgrade / uninstall
│   │       ├── config.rs         #   YAML config + defaults
│   │       ├── server.rs         #   Control channel, yamux, subdomain routing
│   │       ├── registry.rs       #   Client registry (DashMap)
│   │       ├── proxy.rs          #   HTTP reverse proxy
│   │       ├── dashboard.rs      #   REST API + embedded UI
│   │       └── metrics.rs        #   Metrics + rate limiter
│   │
│   └── zo-tunnel-client/         # Client binary
│       └── src/
│           ├── main.rs           #   CLI: connect / upgrade / uninstall
│           ├── config.rs         #   YAML config
│           └── client.rs         #   Auth, yamux, stream proxy
│
├── configs/                      # Example YAML configs
├── web/                          # Dashboard UI (HTML/CSS/JS)
└── scripts/                      # Install, build, test scripts
```

---

## 🧰 Tech Stack

| Crate | Purpose |
|---|---|
| [tokio](https://tokio.rs/) | Async runtime |
| [yamux](https://docs.rs/yamux) | TCP multiplexing |
| [hyper](https://hyper.rs/) | HTTP/1.1 reverse proxy |
| [axum](https://docs.rs/axum) | Dashboard REST API |
| [tokio-rustls](https://docs.rs/tokio-rustls) | TLS support |
| [clap](https://docs.rs/clap) | CLI with subcommands |
| [serde](https://serde.rs/) + serde_yaml | Config serialization |
| [dashmap](https://docs.rs/dashmap) | Concurrent client registry |
| [tracing](https://docs.rs/tracing) | Structured async logging |

---

## 🏗️ Build from source

```bash
git clone https://github.com/Zobite/zo-tunnel.git && cd zo-tunnel
cargo build --release
# → target/release/zo-tunnel-server
# → target/release/zo-tunnel-client
```

## 🧪 Testing

```bash
cargo test --workspace                              # Unit tests
cargo build --release && bash scripts/e2e_test.sh   # End-to-end
```

---

## 🤝 Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## 📄 License

MIT — see [LICENSE](LICENSE) for details.

---

**Built with ❤️ and 🦀 Rust**
