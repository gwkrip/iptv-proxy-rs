# ⚡ iptv-proxy-rs

> High-performance IPTV proxy written in Rust.  
> Bridges your player ↔ upstream streams — handles HLS, DASH, headers, and **ClearKey DRM** automatically.  
> **No KODIPROP needed on the client side.**

[![CI](https://github.com/YOUR_USERNAME/iptv-proxy-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_USERNAME/iptv-proxy-rs/actions/workflows/ci.yml)
[![Release](https://github.com/YOUR_USERNAME/iptv-proxy-rs/actions/workflows/release.yml/badge.svg)](https://github.com/YOUR_USERNAME/iptv-proxy-rs/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## ✨ Features

| Feature | Details |
|---------|---------|
| 🚀 **Async / zero-blocking** | Built on Tokio + Axum — handles thousands of concurrent connections |
| 📺 **HLS proxy** | Fetches `.m3u8` manifests, rewrites all URLs to go through proxy |
| 📺 **DASH proxy** | Fetches `.mpd` manifests, rewrites `BaseURL`, `SegmentTemplate`, `initialization` |
| 🔐 **ClearKey DRM** | Reads `license_key=KID:KEY` from playlist, serves a local W3C JWK license endpoint — no KODIPROP |
| 🪄 **Header injection** | `User-Agent`, `Referer` (and more) are set per-channel when proxying upstream |
| 🎯 **Single static binary** | Compiled with musl — no runtime dependencies, runs everywhere |
| 🐳 **Docker** | Multi-stage `scratch` image ~5 MB |
| 🔄 **Segment streaming** | Segments are streamed (not buffered) for minimal memory usage |

---

## 📦 Installation

### Pre-built binaries (recommended)

Download from the [Releases page](https://github.com/YOUR_USERNAME/iptv-proxy-rs/releases):

| Platform | File |
|----------|------|
| Linux x86_64 (static) | `iptv-proxy-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 (Raspberry Pi 4 / servers) | `iptv-proxy-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz` |
| Linux ARMv7 (Raspberry Pi 3) | `iptv-proxy-vX.Y.Z-armv7-unknown-linux-musleabihf.tar.gz` |
| macOS x86_64 | `iptv-proxy-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `iptv-proxy-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `iptv-proxy-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

### Build from source

```bash
# Prerequisites: Rust stable (https://rustup.rs)
git clone https://github.com/YOUR_USERNAME/iptv-proxy-rs
cd iptv-proxy-rs

# Debug build (fast compile)
cargo build

# Optimised release build
cargo build --release

# Static musl binary (Linux)
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

### Docker

```bash
# Pull from GitHub Container Registry
docker pull ghcr.io/YOUR_USERNAME/iptv-proxy-rs:latest

# Run with your playlist
docker run -d \
  -p 8888:8888 \
  -v /path/to/playlist.m3u8:/data/playlist.m3u8:ro \
  ghcr.io/YOUR_USERNAME/iptv-proxy-rs:latest
```

---

## 🚀 Quick Start

```bash
# Place your playlist next to the binary
cp your_playlist.m3u8 playlist.m3u8

# Run (default port 8888)
./iptv-proxy

# Custom options
./iptv-proxy --port 9999 --playlist /srv/tv/channels.m3u8 --log-level debug
```

Then point your player (Kodi, TiviMate, OTTNavigator, VLC…) at:

```
http://YOUR_IP:8888/playlist.m3u8
```

---

## ⚙️ Options

```
USAGE:
    iptv-proxy [OPTIONS]

OPTIONS:
    -p, --port      <PORT>      Listen port          [default: 8888]  [env: PROXY_PORT]
    -b, --bind      <ADDR>      Bind address         [default: 0.0.0.0] [env: PROXY_BIND]
    -f, --playlist  <FILE>      M3U8 playlist path   [default: playlist.m3u8] [env: PROXY_PLAYLIST]
    -t, --timeout   <SECS>      Upstream timeout     [default: 15]    [env: PROXY_TIMEOUT]
        --log-level <LEVEL>     Logging level        [default: info]  [env: RUST_LOG]
    -h, --help                  Print help
    -V, --version               Print version
```

---

## 🔌 API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/playlist.m3u8` | GET | Modified M3U8 playlist — all URLs point to proxy |
| `/stream/:id` | GET | Stream entry point — redirects to `/hls` or `/mpd` |
| `/hls?ctx=…&url=…` | GET | Proxy + rewrite HLS manifest |
| `/mpd?ctx=…&url=…` | GET | Proxy + rewrite DASH manifest with ClearKey injection |
| `/segment?ctx=…&url=…` | GET | Stream proxy for TS / fMP4 segments |
| `/clearkey?ctx=…` | GET/POST | Local W3C ClearKey JWK license server |
| `/channels.json` | GET | Channel list with metadata (JSON) |
| `/status` | GET | Health check + uptime |

---

## 🔐 ClearKey DRM — How It Works

Original playlist line:
```
https://cdn.example.com/stream/index.mpd|license_type=clearkey&license_key=KID_HEX:KEY_HEX&User-Agent=...
```

What the proxy does:
1. Parses `KID` and `KEY` from the playlist (never sent to client)
2. When client requests `/stream/N`, proxy fetches the MPD with correct headers
3. Proxy rewrites `<ContentProtection>` to point to `http://proxy/clearkey?ctx=<encoded>`
4. When player requests the license, proxy responds with a valid W3C JWK object:
   ```json
   {"keys":[{"kty":"oct","kid":"<base64url_kid>","k":"<base64url_key>"}],"type":"temporary"}
   ```
5. Player decrypts and plays — **no KODIPROP needed** ✅

---

## 🛠️ Development

```bash
# Run tests
cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# Format
cargo fmt

# Run benchmarks
cargo bench

# Watch mode (install cargo-watch first)
cargo watch -x run
```

### Project structure

```
src/
  main.rs       # Server bootstrap
  config.rs     # CLI / environment config (clap)
  playlist.rs   # M3U8 parser + Channel struct
  rewriter.rs   # HLS / DASH manifest URL rewriter + ProxyCtx encoding
  clearkey.rs   # ClearKey JWK license server logic
  proxy.rs      # Upstream HTTP fetch helpers (reqwest)
  handlers.rs   # All Axum route handlers
  state.rs      # Shared AppState (Arc<Vec<Channel>> + reqwest::Client)
benches/
  parsing.rs    # Criterion benchmarks
.github/
  workflows/
    ci.yml      # Fmt + Clippy + Tests + Audit on every push/PR
    release.yml # Cross-platform builds + Docker on tag push
```

---

## 🐳 Docker Compose

```yaml
version: "3.8"
services:
  iptv-proxy:
    image: ghcr.io/YOUR_USERNAME/iptv-proxy-rs:latest
    restart: unless-stopped
    ports:
      - "8888:8888"
    volumes:
      - ./playlist.m3u8:/data/playlist.m3u8:ro
    environment:
      PROXY_PORT: 8888
      PROXY_TIMEOUT: 15
      RUST_LOG: info
```

---

## 📋 Supported Formats

| Format | Support |
|--------|---------|
| HLS (`.m3u8`) | ✅ Full rewrite |
| DASH (`.mpd`) | ✅ Full rewrite |
| ClearKey DRM | ✅ Built-in license server |
| Widevine DRM | ⚠️ Stream proxied, DRM requires CDM hardware |
| TS segments | ✅ Streaming proxy |
| fMP4 segments | ✅ Streaming proxy |
| Custom User-Agent | ✅ Per-channel injection |
| Custom Referer | ✅ Per-channel injection |

---

## 📜 License

[MIT](LICENSE)
