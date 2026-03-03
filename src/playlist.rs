use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// A single IPTV channel parsed from the M3U8 playlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id:           usize,
    pub extinf:       String,
    pub stream_url:   String,
    /// HTTP headers to inject when fetching the upstream (User-Agent, Referer…)
    pub headers:      HashMap<String, String>,
    /// ClearKey DRM keys:  kid_hex → key_hex
    pub clear_keys:   HashMap<String, String>,
    pub license_type: Option<String>,
    pub license_url:  Option<String>,
}

impl Channel {
    /// Returns true when the stream is a DASH manifest
    pub fn is_dash(&self) -> bool {
        let u = &self.stream_url;
        u.contains(".mpd") || u.ends_with("/manifest")
    }
}

// ─── Parser ──────────────────────────────────────────────────────────────────

pub fn parse_playlist(content: &str) -> Vec<Channel> {
    let mut channels  = Vec::new();
    let mut cur_info: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("#EXTINF") {
            cur_info = Some(line.to_string());
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(info) = cur_info.take() {
                channels.push(parse_channel(channels.len(), info, line));
            }
        }
    }

    channels
}

fn parse_channel(id: usize, extinf: String, raw: &str) -> Channel {
    // Split on the first `|` → stream URL + param block
    let (stream_url, param_block) = match raw.splitn(2, '|').collect::<Vec<_>>()[..] {
        [url]        => (url.trim().to_string(), String::new()),
        [url, rest]  => (url.trim().to_string(), rest.to_string()),
        _            => (raw.trim().to_string(), String::new()),
    };

    let mut headers:      HashMap<String, String> = HashMap::new();
    let mut clear_keys:   HashMap<String, String> = HashMap::new();
    let mut license_type: Option<String>           = None;
    let mut license_url:  Option<String>           = None;

    if !param_block.is_empty() {
        // Normalize: replace remaining `|` with `&`
        let flat = param_block.replace('|', "&");
        parse_params(&flat, &mut headers, &mut clear_keys, &mut license_type, &mut license_url);
    }

    Channel { id, extinf, stream_url, headers, clear_keys, license_type, license_url }
}

/// Parse the `key=value&key=value` block that follows the `|` in a stream line.
///
/// The tricky parts:
///  - Values can contain `=` (e.g. `User-Agent=Mozilla/5.0 ...`)
///  - Values can contain `&` inside URLs
///  - `User-Agent=referrer=https://…` means the UA IS the referer
///  - `license_key=KID:KEY` is a ClearKey pair
///  - `license_key=https://…` is a Widevine license URL
fn parse_params(
    flat:         &str,
    headers:      &mut HashMap<String, String>,
    clear_keys:   &mut HashMap<String, String>,
    license_type: &mut Option<String>,
    license_url:  &mut Option<String>,
) {
    // We split into tokens at `&` boundaries, but only when the token after
    // the `&` starts with a *known* key prefix.
    const KNOWN: &[&str] = &[
        "user-agent=", "referer=", "referrer=",
        "license_type=", "license_key=",
    ];

    let tokens = smart_split(flat, KNOWN);

    for token in tokens {
        let token = token.trim();
        if let Some(val) = strip_prefix_ci(token, "license_type=") {
            *license_type = Some(val.to_string());
        } else if let Some(val) = strip_prefix_ci(token, "license_key=") {
            parse_license_key(val, clear_keys, license_url);
        } else if let Some(val) = strip_prefix_ci(token, "user-agent=") {
            parse_user_agent(val, headers);
        } else if let Some(val) = strip_prefix_ci(token, "referrer=")
                               .or_else(|| strip_prefix_ci(token, "referer=")) {
            let v = val.trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() { headers.insert("referer".into(), v); }
        }
    }
}

fn parse_license_key(
    val:        &str,
    clear_keys: &mut HashMap<String, String>,
    license_url: &mut Option<String>,
) {
    if val.starts_with("http://") || val.starts_with("https://") {
        *license_url = Some(val.to_string());
        return;
    }
    // Expect  32-hex:32-hex
    let parts: Vec<&str> = val.splitn(2, ':').collect();
    if parts.len() == 2 {
        let kid = parts[0].trim().to_lowercase();
        let key = parts[1].trim().to_lowercase();
        if kid.len() == 32 && key.len() == 32
            && kid.chars().all(|c| c.is_ascii_hexdigit())
            && key.chars().all(|c| c.is_ascii_hexdigit())
        {
            clear_keys.insert(kid, key);
            return;
        }
    }
    // Fallback: treat as URL
    *license_url = Some(val.to_string());
}

fn parse_user_agent(val: &str, headers: &mut HashMap<String, String>) {
    // Patterns: "User-Agent=referrer=https://…"
    let lower = val.to_lowercase();
    if lower.starts_with("referrer=") || lower.starts_with("referer=") {
        let inner = &val[val.find('=').map(|i| i + 1).unwrap_or(0)..];
        let inner = inner.trim_matches('"').trim_matches('\'');
        if !inner.is_empty() {
            headers.insert("referer".into(), inner.to_string());
        }
    } else if !val.is_empty() {
        headers.insert("user-agent".into(), val.to_string());
    }
}

/// Split `s` at `&` only when the text after `&` starts with one of `known` (case-insensitive).
fn smart_split<'a>(s: &'a str, known: &[&str]) -> Vec<&'a str> {
    let mut result = Vec::new();
    let mut start = 0usize;
    let s_lower = s.to_lowercase();

    for (i, _) in s.char_indices().filter(|(_, c)| *c == '&') {
        let after_lower = &s_lower[i + 1..];
        if known.iter().any(|k| after_lower.starts_with(k)) {
            result.push(&s[start..i]);
            start = i + 1;
        }
    }
    result.push(&s[start..]);
    result
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() { return None; }
    let head = &s[..prefix.len()];
    if head.to_lowercase() == prefix.to_lowercase() {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clearkey_line() {
        let raw = "https://example.com/stream/index.mpd\
            |license_type=clearkey\
            &license_key=c3004565365a42d08e3bde39a516d64e:dbfdc0967cfbbed01dba730c99d9c14a\
            &User-Agent=referrer=https://www.visionplus.id/";
        let ch = parse_channel(0, "#EXTINF:-1".into(), raw);
        assert_eq!(ch.license_type.as_deref(), Some("clearkey"));
        assert_eq!(
            ch.clear_keys.get("c3004565365a42d08e3bde39a516d64e"),
            Some(&"dbfdc0967cfbbed01dba730c99d9c14a".to_string())
        );
        assert_eq!(ch.headers.get("referer").map(|s| s.as_str()),
            Some("https://www.visionplus.id/"));
    }

    #[test]
    fn test_hls_user_agent() {
        let raw = "https://example.com/live.m3u8\
            |User-Agent=Mozilla/5.0 (Windows NT 10.0) Chrome/109.0";
        let ch = parse_channel(0, "#EXTINF:-1".into(), raw);
        assert_eq!(
            ch.headers.get("user-agent").map(|s| s.as_str()),
            Some("Mozilla/5.0 (Windows NT 10.0) Chrome/109.0")
        );
    }

    #[test]
    fn test_parse_playlist_count() {
        let m3u = "#EXTM3U\n\
            #EXTINF:-1 group-title=\"Test\",Channel 1\n\
            https://example.com/ch1.m3u8\n\
            #EXTINF:-1 group-title=\"Test\",Channel 2\n\
            https://example.com/ch2.mpd|license_type=clearkey&license_key=aabb:ccdd";
        // license_key has 4-char segments — won't match 32 hex, but parse shouldn't panic
        let channels = parse_playlist(m3u);
        assert_eq!(channels.len(), 2);
        assert!(!channels[0].is_dash());
        assert!(channels[1].is_dash());
    }
}
