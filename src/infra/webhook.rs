use crate::error::Result;
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use tracing::warn;

/// Fires a JSON POST to a Home Assistant (or generic) webhook URL.
/// Silently logs on failure — never crashes the sentry loop.
pub fn fire_webhook(url: &str, payload: &Value) -> Result<()> {
    if url.is_empty() {
        return Ok(());
    }

    let client = match Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(e) => {
            warn!("Webhook client build failed: {e}");
            return Ok(());
        }
    };

    match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
    {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => warn!("Webhook non-success: {}", resp.status()),
        Err(e) => warn!("Webhook dispatch failed: {e}"),
    }

    Ok(())
}
