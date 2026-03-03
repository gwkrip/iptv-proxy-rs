use std::collections::HashMap;
use anyhow::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

// ─── W3C ClearKey JWK types ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ClearKeyLicenseRequest {
    pub kids: Option<Vec<String>>,
    #[serde(rename = "type")]
    pub request_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwkKey {
    pub kty: String,
    pub kid: String,
    pub k:   String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClearKeyLicenseResponse {
    pub keys: Vec<JwkKey>,
    #[serde(rename = "type")]
    pub response_type: String,
}

// ─── License generation ──────────────────────────────────────────────────────

/// Build a ClearKey JWK license response given:
///  - `clear_keys`:      KID_hex → KEY_hex  (from playlist)
///  - `requested_kids`:  base64url-encoded KIDs from the player request
///
/// Returns all matching keys; falls back to returning all keys if none match.
pub fn build_license(
    clear_keys:     &HashMap<String, String>,
    requested_kids: &[String],
) -> Result<ClearKeyLicenseResponse> {
    let mut keys: Vec<JwkKey> = Vec::new();

    if !requested_kids.is_empty() {
        for b64_kid in requested_kids {
            let kid_hex = b64url_to_hex(b64_kid);
            if let Some(key_hex) = clear_keys.get(&kid_hex) {
                keys.push(make_jwk(b64_kid, key_hex));
            }
        }
    }

    // Fallback: return all keys when player doesn't specify KIDs or none matched
    if keys.is_empty() {
        for (kid_hex, key_hex) in clear_keys {
            let kid_b64 = hex_to_b64url(kid_hex);
            keys.push(make_jwk(&kid_b64, key_hex));
        }
    }

    Ok(ClearKeyLicenseResponse {
        keys,
        response_type: "temporary".to_string(),
    })
}

fn make_jwk(kid_b64url: &str, key_hex: &str) -> JwkKey {
    JwkKey {
        kty: "oct".to_string(),
        kid: kid_b64url.to_string(),
        k:   hex_to_b64url(key_hex),
    }
}

fn hex_to_b64url(hex: &str) -> String {
    let bytes = hex_decode(hex);
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn b64url_to_hex(b64: &str) -> String {
    URL_SAFE_NO_PAD
        .decode(b64)
        .map(|b| b.iter().map(|byte| format!("{:02x}", byte)).collect())
        .unwrap_or_default()
}

fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_license_with_known_kid() {
        let kid_hex = "c3004565365a42d08e3bde39a516d64e";
        let key_hex = "dbfdc0967cfbbed01dba730c99d9c14a";
        let mut keys = HashMap::new();
        keys.insert(kid_hex.to_string(), key_hex.to_string());

        // Encode the KID as the player would
        let kid_b64 = hex_to_b64url(kid_hex);
        let resp = build_license(&keys, &[kid_b64.clone()]).unwrap();

        assert_eq!(resp.keys.len(), 1);
        assert_eq!(resp.keys[0].kty, "oct");
        assert_eq!(resp.keys[0].kid, kid_b64);
        // Verify the key roundtrips
        let decoded = URL_SAFE_NO_PAD.decode(&resp.keys[0].k).unwrap();
        let back_hex: String = decoded.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(back_hex, key_hex);
    }

    #[test]
    fn test_build_license_fallback_all_keys() {
        let mut keys = HashMap::new();
        keys.insert("aabbccdd".repeat(4)[..32].to_string(), "11223344".repeat(4)[..32].to_string());

        // Empty requested KIDs → return all
        let resp = build_license(&keys, &[]).unwrap();
        assert_eq!(resp.keys.len(), 1);
    }

    #[test]
    fn test_hex_b64_roundtrip() {
        let hex = "c3004565365a42d08e3bde39a516d64e";
        let b64 = hex_to_b64url(hex);
        let back = b64url_to_hex(&b64);
        assert_eq!(back, hex);
    }
}
