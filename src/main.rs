mod config;
mod clearkey;
mod handlers;
mod playlist;
mod proxy;
mod rewriter;
mod state;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    config::Config,
    handlers::{
        get_channels_json, get_clearkey, get_hls, get_mpd, get_playlist,
        get_segment, get_status, get_stream, options_handler,
    },
    playlist::parse_playlist,
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── CLI + env config ────────────────────────────────────────────────────
    let cfg = Config::parse_args();

    // ── Logging ─────────────────────────────────────────────────────────────
    let filter = EnvFilter::try_from_env("RUST_LOG")
        .or_else(|_| EnvFilter::try_new(&cfg.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // ── Load playlist ───────────────────────────────────────────────────────
    let raw = std::fs::read_to_string(&cfg.playlist)
        .map_err(|e| anyhow::anyhow!("Cannot read playlist '{}': {}", cfg.playlist.display(), e))?;

    let channels = parse_playlist(&raw);
    info!(count = channels.len(), file = %cfg.playlist.display(), "Playlist loaded");

    if channels.is_empty() {
        warn!("No channels found in playlist — check format");
    }

    // ── HTTP client (shared, pooled) ────────────────────────────────────────
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.timeout))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .tcp_keepalive(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .use_rustls_tls()
        .build()?;

    let state = AppState {
        channels: Arc::new(channels),
        http_client,
        timeout_secs: cfg.timeout,
    };

    // ── CORS ─────────────────────────────────────────────────────────────────
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // ── Router ───────────────────────────────────────────────────────────────
    let app = Router::new()
        // ── Playlist / discovery ──────────────────────────────────────────
        .route("/",                  get(get_playlist))
        .route("/playlist.m3u8",     get(get_playlist))
        .route("/channels.json",     get(get_channels_json))
        .route("/status",            get(get_status))
        // ── Stream entry points ───────────────────────────────────────────
        .route("/stream/:id",        get(get_stream))
        // ── Manifest proxies ──────────────────────────────────────────────
        .route("/hls",               get(get_hls))
        .route("/mpd",               get(get_mpd))
        // ── Segment proxy ─────────────────────────────────────────────────
        .route("/segment",           get(get_segment))
        // ── ClearKey license server ───────────────────────────────────────
        .route("/clearkey",          post(get_clearkey).get(get_clearkey))
        // ── OPTIONS preflight ─────────────────────────────────────────────
        .route("/*path",             axum::routing::options(options_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // ── Bind & serve ─────────────────────────────────────────────────────────
    let addr: SocketAddr = format!("{}:{}", cfg.bind, cfg.port).parse()?;

    let banner = format!(
        r#"
╔══════════════════════════════════════════════════════════╗
║           IPTV PROXY  (Rust / ⚡ ultra-fast)            ║
╠══════════════════════════════════════════════════════════╣
║  Listen   : http://{}
║  Playlist : http://{}:{}/playlist.m3u8
║  Status   : http://{}:{}/status
╚══════════════════════════════════════════════════════════╝
→ Tidak perlu KODIPROP – proxy handles DRM ClearKey!
"#,
        addr, cfg.bind, cfg.port, cfg.bind, cfg.port
    );
    println!("{}", banner);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
