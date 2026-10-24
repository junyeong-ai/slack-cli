use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

use super::core::SlackCore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// What is shared along with an uploaded file. Unset fields are left to
/// Slack: the title defaults to the file name.
#[derive(Debug, Default)]
pub struct UploadOptions {
    pub title: Option<String>,
    pub initial_comment: Option<String>,
    pub thread_ts: Option<String>,
    pub alt_txt: Option<String>,
}

pub struct SlackFileClient {
    core: Arc<SlackCore>,
}

impl SlackFileClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    /// Slack's external upload: reserve an upload URL, stream the file there,
    /// then complete the upload into `channel`. A reservation that is never
    /// completed is discarded by Slack, so a failure leaves nothing behind.
    pub async fn upload(
        &self,
        channel: &str,
        path: &Path,
        options: UploadOptions,
    ) -> Result<UploadedFile> {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("{} does not name a file", path.display()))?;
        let file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("failed to open {}", path.display()))?;
        let length = file
            .metadata()
            .await
            .with_context(|| format!("failed to read {}", path.display()))?
            .len();

        let mut reservation = json!({ "filename": filename, "length": length });
        if let Some(alt_txt) = options.alt_txt {
            reservation["alt_txt"] = json!(alt_txt);
        }
        let reserved = self
            .core
            .api_call("files.getUploadURLExternal", reservation)
            .await?;
        let upload_url = reserved["upload_url"]
            .as_str()
            .context("Missing upload_url in files.getUploadURLExternal response")?;
        let file_id = reserved["file_id"]
            .as_str()
            .context("Missing file_id in files.getUploadURLExternal response")?;

        self.core.send_upload(upload_url, file.into()).await?;

        let mut shared = json!({ "id": file_id });
        if let Some(title) = options.title {
            shared["title"] = json!(title);
        }
        let mut completion = json!({
            "files": Value::Array(vec![shared]).to_string(),
            "channel_id": channel,
        });
        if let Some(comment) = options.initial_comment {
            completion["initial_comment"] = json!(comment);
        }
        if let Some(thread_ts) = options.thread_ts {
            completion["thread_ts"] = json!(thread_ts);
        }
        let mut response = self
            .core
            .api_call_in_conversation("files.completeUploadExternal", completion, "channel_id")
            .await?;
        let files = response
            .get_mut("files")
            .map(Value::take)
            .context("Missing files in files.completeUploadExternal response")?;
        serde_json::from_value::<Vec<UploadedFile>>(files)?
            .into_iter()
            .next()
            .context("files.completeUploadExternal returned no file")
    }
}
