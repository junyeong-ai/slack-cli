use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::json;

use super::core::SlackCore;

pub struct SlackAppsClient {
    core: Arc<SlackCore>,
}

impl SlackAppsClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    /// `apps.connections.open` — the WebSocket URL a Socket Mode connection is
    /// established on.
    ///
    /// The URL is single-use and short-lived: Slack expects it to be dialled
    /// immediately, and every reconnect asks for a fresh one. It is the only
    /// method authorized by the app-level token rather than an installation.
    pub async fn connection(&self) -> Result<String> {
        let response = self
            .core
            .api_call("apps.connections.open", json!({}))
            .await?;
        response
            .get("url")
            .and_then(|url| url.as_str())
            .map(ToOwned::to_owned)
            .context("apps.connections.open returned no url")
    }
}
