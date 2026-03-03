use std::path::PathBuf;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name    = "iptv-proxy",
    version,
    author,
    about   = "High-performance IPTV proxy — HLS/DASH rewriting + ClearKey DRM",
    long_about = None,
)]
pub struct Config {
    /// TCP port to listen on
    #[arg(short, long, default_value = "8888", env = "PROXY_PORT")]
    pub port: u16,

    /// Bind address
    #[arg(short, long, default_value = "0.0.0.0", env = "PROXY_BIND")]
    pub bind: String,

    /// Path to M3U8 playlist file
    #[arg(short = 'f', long, default_value = "playlist.m3u8", env = "PROXY_PLAYLIST")]
    pub playlist: PathBuf,

    /// Upstream request timeout in seconds
    #[arg(short, long, default_value = "15", env = "PROXY_TIMEOUT")]
    pub timeout: u64,

    /// Log level: trace | debug | info | warn | error
    #[arg(long, default_value = "info", env = "RUST_LOG")]
    pub log_level: String,
}

impl Config {
    pub fn parse_args() -> Self {
        Config::parse()
    }
}
