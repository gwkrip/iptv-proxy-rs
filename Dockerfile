# ─── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1.81-slim AS builder

# Install musl for a fully static binary
RUN apt-get update && apt-get install -y musl-tools pkg-config && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build

# Cache dependencies separately from source
COPY Cargo.toml Cargo.lock ./
# Trick: create a dummy main to compile deps
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl --locked 2>/dev/null || true
RUN rm src/main.rs

# Build the real binary
COPY src ./src
RUN touch src/main.rs  # invalidate cached dummy
RUN cargo build --release --target x86_64-unknown-linux-musl --locked

# ─── Stage 2: Minimal runtime image ──────────────────────────────────────────
FROM scratch

LABEL org.opencontainers.image.title       = "iptv-proxy"
LABEL org.opencontainers.image.description = "High-performance IPTV proxy (Rust)"
LABEL org.opencontainers.image.source      = "https://github.com/YOUR_USERNAME/iptv-proxy-rs"
LABEL org.opencontainers.image.licenses    = "MIT"

# Copy static binary only
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/iptv-proxy /iptv-proxy

# Playlist is mounted at runtime
VOLUME ["/data"]
WORKDIR /data

EXPOSE 8888

ENV PROXY_PORT=8888 \
    PROXY_BIND=0.0.0.0 \
    PROXY_PLAYLIST=/data/playlist.m3u8 \
    PROXY_TIMEOUT=15 \
    RUST_LOG=info

ENTRYPOINT ["/iptv-proxy"]
