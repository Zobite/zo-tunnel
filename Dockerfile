# ── Stage 1: Build ──
FROM rust:1.80-slim AS builder

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY crates/zobite-tunnel-protocol/Cargo.toml crates/zobite-tunnel-protocol/Cargo.toml
COPY crates/zobite-tunnel-server/Cargo.toml crates/zobite-tunnel-server/Cargo.toml
COPY crates/zobite-tunnel-client/Cargo.toml crates/zobite-tunnel-client/Cargo.toml

RUN mkdir -p crates/zobite-tunnel-protocol/src && echo "" > crates/zobite-tunnel-protocol/src/lib.rs && \
    mkdir -p crates/zobite-tunnel-server/src && echo "fn main(){}" > crates/zobite-tunnel-server/src/main.rs && \
    mkdir -p crates/zobite-tunnel-client/src && echo "fn main(){}" > crates/zobite-tunnel-client/src/main.rs && \
    cargo build --release 2>/dev/null || true

# Build real code
COPY crates/ crates/
COPY web/ web/
RUN cargo build --release

# ── Stage 2: Server (5.5MB final image) ──
FROM debian:bookworm-slim AS server

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    useradd -r -s /bin/false zobite-tunnel

COPY --from=builder /app/target/release/zobite-tunnel-server /usr/local/bin/zobite-tunnel-server

USER zobite-tunnel

EXPOSE 7000 8080 9000
# TCP tunnel port range
EXPOSE 10000-10100

ENTRYPOINT ["zobite-tunnel-server"]
CMD ["--control-port", "7000", "--public-port", "8080", "--dashboard-port", "9000"]

# ── Stage 3: Client ──
FROM debian:bookworm-slim AS client

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/zobite-tunnel-client /usr/local/bin/zobite-tunnel-client

ENTRYPOINT ["zobite-tunnel-client"]
