# ── Stage 1: Build ──
FROM rust:1.85-slim AS builder

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY crates/zo-tunnel-protocol/Cargo.toml crates/zo-tunnel-protocol/Cargo.toml
COPY crates/zo-tunnel-server/Cargo.toml crates/zo-tunnel-server/Cargo.toml
COPY crates/zo-tunnel-client/Cargo.toml crates/zo-tunnel-client/Cargo.toml

RUN mkdir -p crates/zo-tunnel-protocol/src && echo "" > crates/zo-tunnel-protocol/src/lib.rs && \
    mkdir -p crates/zo-tunnel-server/src && echo "fn main(){}" > crates/zo-tunnel-server/src/main.rs && \
    mkdir -p crates/zo-tunnel-client/src && echo "fn main(){}" > crates/zo-tunnel-client/src/main.rs && \
    cargo build --release 2>/dev/null || true

# Build real code
COPY crates/ crates/
COPY web/ web/
RUN cargo build --release

# ── Stage 2: Server (5.5MB final image) ──
FROM debian:bookworm-slim AS server

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    useradd -r -s /bin/false zo-tunnel

COPY --from=builder /app/target/release/zo-tunnel-server /usr/local/bin/zo-tunnel-server

USER zo-tunnel

EXPOSE 6200 6210 6220
# TCP tunnel port range
EXPOSE 10000-10100

ENTRYPOINT ["zo-tunnel-server"]
CMD ["--control-port", "6200", "--public-port", "6210", "--dashboard-port", "6220"]

# ── Stage 3: Client ──
FROM debian:bookworm-slim AS client

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/zo-tunnel-client /usr/local/bin/zo-tunnel-client

ENTRYPOINT ["zo-tunnel-client"]
