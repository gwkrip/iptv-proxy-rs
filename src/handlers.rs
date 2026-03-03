use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

use crate::{
    clearkey::{build_license, ClearKeyLicenseRequest},
    proxy::{fetch_bytes, fetch_stream},
    rewriter::{rewrite_hls, rewrite_mpd, ProxyCtx},
    state::AppState,
};

// ─── Error helper ─────────────────────────────────────────────────────────────

pub(crate) struct ProxyError(anyhow::Error);

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let msg = self.0.to_string();
        error!(error = %msg, "proxy error");
        (StatusCode::BAD_GATEWAY, msg).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ProxyError {
    fn from(e: E) -> Self { ProxyError(e.into()) }
}

type HandlerResult<T> = Result<T, ProxyError>;

// ─── Query structs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ManifestQuery {
    pub ctx: Option<String>,
    pub url: Option<String>,
}

#[derive(Deserialize)]
pub struct SegmentQuery {
    pub ctx: Option<String>,
    pub url: Option<String>,
}

#[derive(Deserialize)]
pub struct ClearKeyQuery {
    pub ctx: Option<String>,
}

// ─── /playlist.m3u8 ──────────────────────────────────────────────────────────

pub async fn get_playlist(State(state): State<AppState>, req: axum::http::Request<Body>) -> Response {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8888");
    let proxy_base = format!("http://{}", host);

    let mut lines = vec!["#EXTM3U".to_string(), "".to_string()];
    for ch in state.channels.iter() {
        lines.push(ch.extinf.clone());
        lines.push(format!("{}/stream/{}", proxy_base, ch.id));
    }
    let body = lines.join("\n");

    Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "no-cache, no-store")
        .body(Body::from(body))
        .unwrap()
}

// ─── /stream/:id ─────────────────────────────────────────────────────────────

pub async fn get_stream(
    Path(id): Path<usize>,
    State(state): State<AppState>,
    req: axum::http::Request<Body>,
) -> Response {
    let Some(ch) = state.channels.get(id) else {
        return (StatusCode::NOT_FOUND, "Channel not found").into_response();
    };

    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8888");
    let proxy_base = format!("http://{}", host);

    let ctx = ProxyCtx {
        headers:      ch.headers.clone(),
        clear_keys:   ch.clear_keys.clone(),
        license_type: ch.license_type.clone(),
    };
    let ctx_b64  = ctx.encode();
    let url_enc  = urlencoding::encode(&ch.stream_url).to_string();

    let kind = if ch.is_dash() { "mpd" } else { "hls" };
    let location = format!("{}/{kind}?ctx={ctx_b64}&url={url_enc}", proxy_base);

    debug!(channel_id = id, name = %ch.extinf, kind, "stream redirect");

    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, &location)
        .body(Body::empty())
        .unwrap()
}

// ─── /hls ─────────────────────────────────────────────────────────────────────

pub async fn get_hls(
    Query(q): Query<ManifestQuery>,
    State(state): State<AppState>,
    req: axum::http::Request<Body>,
) -> HandlerResult<Response> {
    let target_url = q.url.as_deref().unwrap_or("").to_string();
    if target_url.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, "Missing ?url=").into_response());
    }
    let ctx_b64  = q.ctx.clone().unwrap_or_default();
    let ctx      = ProxyCtx::decode(&ctx_b64);

    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("localhost:8888");
    let proxy_base = format!("http://{}", host);

    debug!(url = %target_url, "fetching HLS manifest");

    let (_status, resp_headers, body) = fetch_bytes(&state.http_client, &target_url, &ctx.headers).await?;
    let text = String::from_utf8_lossy(&body);

    // If it turned out to be a DASH manifest, redirect
    let ct = resp_headers.get("content-type").map(|s| s.as_str()).unwrap_or("");
    if ct.contains("mpd") || target_url.contains(".mpd") {
        let location = format!("{}/mpd?ctx={}&url={}", proxy_base, ctx_b64, urlencoding::encode(&target_url));
        return Ok(Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, location)
            .body(Body::empty())
            .unwrap());
    }

    let rewritten = rewrite_hls(&text, &target_url, &proxy_base, &ctx_b64);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(rewritten))
        .unwrap())
}

// ─── /mpd ─────────────────────────────────────────────────────────────────────

pub async fn get_mpd(
    Query(q): Query<ManifestQuery>,
    State(state): State<AppState>,
    req: axum::http::Request<Body>,
) -> HandlerResult<Response> {
    let target_url = q.url.as_deref().unwrap_or("").to_string();
    if target_url.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, "Missing ?url=").into_response());
    }
    let ctx_b64 = q.ctx.clone().unwrap_or_default();
    let ctx     = ProxyCtx::decode(&ctx_b64);

    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("localhost:8888");
    let proxy_base = format!("http://{}", host);

    debug!(url = %target_url, clearkey = ctx.has_clear_keys(), "fetching DASH MPD");

    let (_status, resp_headers, body) = fetch_bytes(&state.http_client, &target_url, &ctx.headers).await?;
    let text = String::from_utf8_lossy(&body);

    let rewritten = rewrite_mpd(&text, &target_url, &proxy_base, &ctx, &ctx_b64);

    let content_type = resp_headers
        .get("content-type")
        .map(|s| s.as_str())
        .unwrap_or("application/dash+xml")
        .to_string();

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(rewritten))
        .unwrap())
}

// ─── /segment ─────────────────────────────────────────────────────────────────

pub async fn get_segment(
    Query(q): Query<SegmentQuery>,
    State(state): State<AppState>,
) -> HandlerResult<Response> {
    let target_url = q.url.as_deref().unwrap_or("").to_string();
    if target_url.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, "Missing ?url=").into_response());
    }
    let ctx = ProxyCtx::decode(q.ctx.as_deref().unwrap_or(""));

    debug!(url = %target_url, "streaming segment");

    // Sub-manifest? Forward to HLS handler logic (rare edge-case)
    if target_url.to_lowercase().contains(".m3u8") {
        let (_s, _h, body) = fetch_bytes(&state.http_client, &target_url, &ctx.headers).await?;
        return Ok(Response::builder()
            .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
            .body(Body::from(body))
            .unwrap());
    }

    // Stream the segment directly (TS / fMP4 / WebM) without buffering
    let upstream = fetch_stream(&state.http_client, &target_url, &ctx.headers).await?;

    let content_type = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let content_length = upstream
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let stream = upstream
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, &content_type)
        .header(header::CACHE_CONTROL, "public, max-age=60")
        .header("Access-Control-Allow-Origin", "*");

    if let Some(cl) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, cl);
    }

    Ok(builder.body(Body::from_stream(stream)).unwrap())
}

// ─── /clearkey ────────────────────────────────────────────────────────────────

pub async fn get_clearkey(
    Query(q): Query<ClearKeyQuery>,
    State(_state): State<AppState>,
    body: Bytes,
) -> HandlerResult<Response> {
    let ctx = ProxyCtx::decode(q.ctx.as_deref().unwrap_or(""));

    if ctx.clear_keys.is_empty() {
        warn!("ClearKey request but no keys in ctx");
        return Ok((StatusCode::NOT_FOUND, "No keys available").into_response());
    }

    let request: ClearKeyLicenseRequest = serde_json::from_slice(&body)
        .unwrap_or(ClearKeyLicenseRequest { kids: None, request_type: None });

    let kids = request.kids.unwrap_or_default();
    let license = build_license(&ctx.clear_keys, &kids)?;

    let json = serde_json::to_string(&license)
        .map_err(|e| anyhow::anyhow!("JSON serialization error: {}", e))?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(json))
        .unwrap())
}

// ─── /status ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct StatusResponse<'a> {
    status:   &'a str,
    version:  &'a str,
    channels: usize,
    uptime_s: u64,
}

static START_TIME: once_cell::sync::Lazy<std::time::Instant> =
    once_cell::sync::Lazy::new(std::time::Instant::now);

pub async fn get_status(State(state): State<AppState>) -> Json<StatusResponse<'static>> {
    let _ = *START_TIME; // ensure initialized
    Json(StatusResponse {
        status:   "ok",
        version:  env!("CARGO_PKG_VERSION"),
        channels: state.channels.len(),
        uptime_s: START_TIME.elapsed().as_secs(),
    })
}

// ─── /channels.json ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct ChannelInfo {
    id:      usize,
    name:    String,
    group:   String,
    url:     String,
    kind:    String,
    drm:     String,
    has_key: bool,
}

pub async fn get_channels_json(
    State(state): State<AppState>,
    req: axum::http::Request<Body>,
) -> Json<Vec<ChannelInfo>> {
    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("localhost:8888");
    let proxy_base = format!("http://{}", host);

    let info: Vec<ChannelInfo> = state.channels.iter().map(|ch| {
        let name = extract_extinf_name(&ch.extinf).unwrap_or("Unknown").to_string();
        let group = extract_extinf_attr(&ch.extinf, "group-title").unwrap_or("-").to_string();
        ChannelInfo {
            id:      ch.id,
            name,
            group,
            url:     format!("{}/stream/{}", proxy_base, ch.id),
            kind:    if ch.is_dash() { "DASH".into() } else { "HLS".into() },
            drm:     ch.license_type.clone().unwrap_or_else(|| "none".into()),
            has_key: !ch.clear_keys.is_empty(),
        }
    }).collect();

    Json(info)
}

fn extract_extinf_name(extinf: &str) -> Option<&str> {
    extinf.rsplit(',').next().map(|s| s.trim())
}

fn extract_extinf_attr<'a>(extinf: &'a str, attr: &str) -> Option<&'a str> {
    let search = format!("{}=\"", attr);
    let start = extinf.find(&search)? + search.len();
    let end = extinf[start..].find('"').map(|i| start + i)?;
    Some(&extinf[start..end])
}

// ─── OPTIONS preflight ───────────────────────────────────────────────────────

pub async fn options_handler() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        .header("Access-Control-Allow-Headers", "*")
        .body(Body::empty())
        .unwrap()
}
