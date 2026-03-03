use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Proxy Context (serialized into ?ctx= query param) ───────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyCtx {
    pub headers: HashMap<String, String>,
    pub clear_keys: HashMap<String, String>,
    pub license_type: Option<String>,
}

impl ProxyCtx {
    pub fn encode(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        URL_SAFE_NO_PAD.encode(json.as_bytes())
    }

    pub fn decode(s: &str) -> Self {
        URL_SAFE_NO_PAD
            .decode(s)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    }

    pub fn has_clear_keys(&self) -> bool {
        !self.clear_keys.is_empty()
    }
}

// ─── URL helpers ─────────────────────────────────────────────────────────────

pub fn resolve_url(relative: &str, base: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    // Derive base directory
    let base_dir = base.rfind('/').map(|i| &base[..=i]).unwrap_or(base);

    if relative.starts_with('/') {
        // Absolute path — keep origin
        if let Some(origin_end) = base
            .find("://")
            .and_then(|p| base[p + 3..].find('/').map(|q| p + 3 + q))
        {
            return format!("{}{}", &base[..origin_end], relative);
        }
    }
    // Relative path
    format!("{}{}", base_dir, relative)
}

fn is_manifest_url(url: &str) -> bool {
    let u = url.to_lowercase();
    u.contains(".m3u8") || u.contains("chunklist") || u.contains("playlist")
}

fn proxy_url(kind: &str, raw_url: &str, proxy_base: &str, ctx_b64: &str) -> String {
    format!(
        "{}/{}?ctx={}&url={}",
        proxy_base,
        kind,
        ctx_b64,
        urlencoding::encode(raw_url)
    )
}

// ─── HLS rewriter ────────────────────────────────────────────────────────────

/// Rewrite every URL inside an HLS manifest (.m3u8) so that it points
/// to the local proxy, preserving header/key context.
pub fn rewrite_hls(content: &str, base_url: &str, proxy_base: &str, ctx_b64: &str) -> String {
    content
        .lines()
        .map(|line| rewrite_hls_line(line, base_url, proxy_base, ctx_b64))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rewrite_hls_line(line: &str, base: &str, proxy_base: &str, ctx: &str) -> String {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return line.to_string();
    }

    if trimmed.starts_with('#') {
        // Rewrite URI="..." inside tags (#EXT-X-KEY, #EXT-X-MAP, …)
        static URI_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"URI="([^"]+)""#).unwrap());
        URI_RE
            .replace_all(trimmed, |caps: &regex::Captures| {
                let uri = &caps[1];
                let abs = resolve_url(uri, base);
                // Key URIs → clearkey endpoint; segment maps → segment
                if abs.ends_with(".key") || abs.contains("/key") {
                    format!(r#"URI="{}/clearkey?ctx={}""#, proxy_base, ctx)
                } else {
                    format!(r#"URI="{}""#, proxy_url("segment", &abs, proxy_base, ctx))
                }
            })
            .to_string()
    } else {
        // Bare URL line (segment or sub-manifest)
        let abs = resolve_url(trimmed, base);
        if is_manifest_url(&abs) {
            proxy_url("hls", &abs, proxy_base, ctx)
        } else {
            proxy_url("segment", &abs, proxy_base, ctx)
        }
    }
}

// ─── DASH / MPD rewriter ─────────────────────────────────────────────────────

/// Rewrite a DASH MPD:
///  1. `<BaseURL>` content → proxy/segment
///  2. `initialization="..."` attributes → proxy/segment
///  3. `media="..."` attributes → proxy/segment (template preserved)
///  4. Replace / inject `<ContentProtection>` with local ClearKey license server
pub fn rewrite_mpd(
    content: &str,
    base_url: &str,
    proxy_base: &str,
    ctx: &ProxyCtx,
    ctx_b64: &str,
) -> String {
    let base_dir = base_url
        .rfind('/')
        .map(|i| &base_url[..=i])
        .unwrap_or(base_url);

    let mut out = content.to_string();

    // ── 1. <BaseURL>…</BaseURL> ───────────────────────────────────────────
    static BASE_URL_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?s)<BaseURL[^>]*>(.*?)</BaseURL>").unwrap());
    out = BASE_URL_RE
        .replace_all(&out, |caps: &regex::Captures| {
            let inner = caps[1].trim();
            let abs = resolve_url(inner, base_dir);
            format!(
                "<BaseURL>{}</BaseURL>",
                proxy_url("segment", &abs, proxy_base, ctx_b64)
            )
        })
        .to_string();

    // ── 2. initialization="…" ─────────────────────────────────────────────
    static INIT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"initialization="([^"]+)""#).unwrap());
    out = INIT_RE
        .replace_all(&out, |caps: &regex::Captures| {
            let abs = resolve_url(&caps[1], base_dir);
            format!(
                r#"initialization="{}""#,
                proxy_url("segment", &abs, proxy_base, ctx_b64)
            )
        })
        .to_string();

    // ── 3. media="…" (SegmentTemplate) ────────────────────────────────────
    static MEDIA_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?P<pre>\s)media="(?P<val>[^"]+)""#).unwrap());
    out = MEDIA_RE
        .replace_all(&out, |caps: &regex::Captures| {
            let val = &caps["val"];
            let full = if val.starts_with("http://") || val.starts_with("https://") {
                val.to_string()
            } else {
                format!("{}{}", base_dir, val)
            };
            format!(
                r#"{}media="{}""#,
                &caps["pre"],
                proxy_url("segment", &full, proxy_base, ctx_b64)
            )
        })
        .to_string();

    // ── 4. ClearKey DRM injection ──────────────────────────────────────────
    if ctx.has_clear_keys() {
        // Remove any existing ClearKey ContentProtection blocks
        static CLEARKEY_CP_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(
                r#"(?s)<ContentProtection[^>]*e2719d58-a985-b3c9-781a-b030af78d30e[^>]*>.*?</ContentProtection>"#
            ).unwrap()
        });
        out = CLEARKEY_CP_RE.replace_all(&out, "").to_string();

        // Also handle self-closing
        static CLEARKEY_CP_SC: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r#"<ContentProtection[^>]*e2719d58-a985-b3c9-781a-b030af78d30e[^/]*/>"#)
                .unwrap()
        });
        out = CLEARKEY_CP_SC.replace_all(&out, "").to_string();

        let ck_url = format!("{}/clearkey?ctx={}", proxy_base, ctx_b64);
        let inject = format!(
            r#"<ContentProtection schemeIdUri="urn:uuid:e2719d58-a985-b3c9-781a-b030af78d30e">
      <dashif:Laurl xmlns:dashif="https://dashif.org/CPS">{}</dashif:Laurl>
    </ContentProtection>"#,
            ck_url
        );

        // Inject once per <AdaptationSet>
        static ADAPT_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"(<AdaptationSet\b[^>]*>)").unwrap());
        out = ADAPT_RE
            .replace_all(&out, |caps: &regex::Captures| {
                format!("{}\n    {}", &caps[1], inject)
            })
            .to_string();
    }

    out
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_relative() {
        assert_eq!(
            resolve_url("segment.ts", "https://cdn.example.com/path/index.m3u8"),
            "https://cdn.example.com/path/segment.ts"
        );
    }

    #[test]
    fn test_resolve_absolute_path() {
        assert_eq!(
            resolve_url("/live/stream.ts", "https://cdn.example.com/hls/index.m3u8"),
            "https://cdn.example.com/live/stream.ts"
        );
    }

    #[test]
    fn test_resolve_full_url() {
        let u = "https://other.cdn.com/stream.ts";
        assert_eq!(resolve_url(u, "https://cdn.example.com/"), u);
    }

    #[test]
    fn test_hls_rewrite_tag_uri() {
        let hls = r#"#EXTM3U
#EXT-X-KEY:METHOD=AES-128,URI="https://key.server/key123",IV=0x00
seg001.ts
"#;
        let out = rewrite_hls(
            hls,
            "https://cdn.example.com/stream/",
            "http://proxy:8888",
            "CTX",
        );
        assert!(out.contains("http://proxy:8888/clearkey?ctx=CTX"));
        assert!(out.contains("http://proxy:8888/segment?ctx=CTX&url="));
    }

    #[test]
    fn test_proxy_ctx_roundtrip() {
        let ctx = ProxyCtx {
            headers: [("user-agent".into(), "VLC".into())].into(),
            clear_keys: [("aabbccdd".repeat(4), "11223344".repeat(4))].into(),
            license_type: Some("clearkey".into()),
        };
        let encoded = ctx.encode();
        let decoded = ProxyCtx::decode(&encoded);
        assert_eq!(
            decoded.headers.get("user-agent").map(|s| s.as_str()),
            Some("VLC")
        );
        assert!(decoded.has_clear_keys());
    }
}
