use std::sync::Arc;
use crate::playlist::Channel;

#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub channels:     Arc<Vec<Channel>>,
    pub http_client:  reqwest::Client,
    pub timeout_secs: u64,
}
