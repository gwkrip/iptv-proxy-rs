use std::collections::HashMap;
use anyhow::{anyhow, Result};
use bytes::Bytes;
use reqwest::{Client, Response, StatusCode};

/// Fetch a URL with the given headers, returning the full body as Bytes.
pub async fn fetch_bytes(
    client:  &Client,
    url:     &str,
    headers: &HashMap<String, String>,
) -> Result<(StatusCode, HashMap<String, String>, Bytes)> {
    let mut req = client.get(url);

    req = req.header("Accept",          "*/*")
             .header("Accept-Encoding", "identity")
             .header("Connection",      "keep-alive");

    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }

    let resp = req.send().await
        .map_err(|e| anyhow!("fetch error for {url}: {e}"))?;

    let status = resp.status();
    let resp_headers = extract_headers(&resp);
    let body = resp.bytes().await
        .map_err(|e| anyhow!("body read error: {e}"))?;

    Ok((status, resp_headers, body))
}

/// Fetch a URL and return the raw reqwest::Response for streaming.
pub async fn fetch_stream(
    client:  &Client,
    url:     &str,
    headers: &HashMap<String, String>,
) -> Result<Response> {
    let mut req = client.get(url);

    req = req.header("Accept",          "*/*")
             .header("Accept-Encoding", "identity");

    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }

    req.send().await.map_err(|e| anyhow!("fetch_stream {url}: {e}"))
}

fn extract_headers(resp: &Response) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (k, v) in resp.headers() {
        if let Ok(val) = v.to_str() {
            map.insert(k.to_string(), val.to_string());
        }
    }
    map
}
