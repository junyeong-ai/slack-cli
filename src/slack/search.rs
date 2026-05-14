use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use super::core::SlackCore;

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SearchChannelType {
    #[value(name = "public_channel")]
    PublicChannel,
    #[value(name = "private_channel")]
    PrivateChannel,
    Mpim,
    Im,
}

impl SearchChannelType {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::PublicChannel => "public_channel",
            Self::PrivateChannel => "private_channel",
            Self::Mpim => "mpim",
            Self::Im => "im",
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SearchContentType {
    Messages,
    Files,
    Channels,
    Users,
}

impl SearchContentType {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Files => "files",
            Self::Channels => "channels",
            Self::Users => "users",
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SearchSort {
    Score,
    Timestamp,
}

impl SearchSort {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Score => "score",
            Self::Timestamp => "timestamp",
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SearchSortDirection {
    Asc,
    Desc,
}

impl SearchSortDirection {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    pub channel_types: Vec<SearchChannelType>,
    pub content_types: Vec<SearchContentType>,
    pub context_channel_id: Option<String>,
    pub include_archived_channels: bool,
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub include_bots: bool,
    pub disable_semantic_search: bool,
    pub sort: SearchSort,
    pub sort_dir: SearchSortDirection,
    pub include_context_messages: bool,
    pub include_message_blocks: bool,
    pub highlight: bool,
}

impl SearchOptions {
    pub const MAX_LIMIT: usize = 100;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCapabilities {
    pub is_ai_search_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchContextResponse {
    results: SearchResults,
    response_metadata: Option<SearchResponseMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchResponseMetadata {
    #[serde(default)]
    next_cursor: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResults {
    #[serde(default)]
    pub messages: Vec<SearchMessageResult>,
    #[serde(default)]
    pub files: Vec<SearchFileResult>,
    #[serde(default)]
    pub channels: Vec<SearchChannelResult>,
    #[serde(default)]
    pub users: Vec<SearchUserResult>,
}

impl SearchResults {
    fn extend(&mut self, other: Self) {
        self.messages.extend(other.messages);
        self.files.extend(other.files);
        self.channels.extend(other.channels);
        self.users.extend(other.users);
    }

    fn total_len(&self) -> usize {
        self.messages.len() + self.files.len() + self.channels.len() + self.users.len()
    }

    fn truncate(&mut self, limit: usize) {
        let mut remaining = limit;

        truncate_vec(&mut self.messages, &mut remaining);
        truncate_vec(&mut self.files, &mut remaining);
        truncate_vec(&mut self.channels, &mut remaining);
        truncate_vec(&mut self.users, &mut remaining);
    }
}

fn truncate_vec<T>(values: &mut Vec<T>, remaining: &mut usize) {
    if values.len() > *remaining {
        values.truncate(*remaining);
        *remaining = 0;
    } else {
        *remaining -= values.len();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMessageResult {
    #[serde(rename = "message_ts")]
    pub ts: String,
    #[serde(default)]
    #[serde(rename = "content")]
    pub text: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub channel_name: Option<String>,
    #[serde(default)]
    pub author_user_id: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub is_author_bot: bool,
    #[serde(default)]
    pub blocks: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub context_messages: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFileResult {
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub file_type: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub author_user_id: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub date_created: Option<i64>,
    #[serde(default)]
    pub date_updated: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchChannelResult {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub creator_user_id: Option<String>,
    #[serde(default)]
    pub creator_name: Option<String>,
    #[serde(default)]
    pub date_created: Option<i64>,
    #[serde(default)]
    pub date_updated: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchUserResult {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub full_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub profile_pic_permalink: Option<String>,
}

pub struct SlackSearchClient {
    core: Arc<SlackCore>,
}

impl SlackSearchClient {
    pub fn new(core: Arc<SlackCore>) -> Self {
        Self { core }
    }

    pub async fn capabilities(&self) -> Result<SearchCapabilities> {
        let response = self
            .core
            .api_call("assistant.search.info", json!({}))
            .await?;

        Ok(serde_json::from_value(response)?)
    }

    pub async fn search(&self, query: &str, options: &SearchOptions) -> Result<SearchResults> {
        let limit = options.limit.clamp(1, SearchOptions::MAX_LIMIT);
        let mut params = build_request_params(query, options);

        let mut results = SearchResults::default();
        let mut cursor: Option<String> = None;

        loop {
            let remaining = limit.saturating_sub(results.total_len());
            let page_size = remaining.clamp(1, PAGE_SIZE);
            params["limit"] = json!(page_size);

            if let Some(cursor) = &cursor {
                params["cursor"] = json!(cursor);
            }

            let response = self
                .core
                .api_call("assistant.search.context", params.clone())
                .await?;

            let response: SearchContextResponse = serde_json::from_value(response)?;
            let next_cursor = response
                .response_metadata
                .map(|m| m.next_cursor)
                .filter(|c| !c.is_empty());
            results.extend(response.results);

            cursor = next_cursor;
            if cursor.is_none() || results.total_len() >= limit {
                break;
            }
        }

        results.truncate(limit);
        Ok(results)
    }
}

fn build_request_params(query: &str, options: &SearchOptions) -> Value {
    let channel_types: Vec<&'static str> = options
        .channel_types
        .iter()
        .map(|t| t.as_api_str())
        .collect();
    let content_types: Vec<&'static str> = options
        .content_types
        .iter()
        .map(|t| t.as_api_str())
        .collect();

    let mut params = json!({
        "query": query,
        "channel_types": channel_types,
        "content_types": content_types,
        "include_archived_channels": options.include_archived_channels,
        "include_bots": options.include_bots,
        "disable_semantic_search": options.disable_semantic_search,
        "sort": options.sort.as_api_str(),
        "sort_dir": options.sort_dir.as_api_str(),
        "include_context_messages": options.include_context_messages,
        "include_message_blocks": options.include_message_blocks,
        "highlight": options.highlight,
    });

    if let Some(channel_id) = &options.context_channel_id {
        params["context_channel_id"] = json!(channel_id);
    }
    if let Some(before) = options.before {
        params["before"] = json!(before);
    }
    if let Some(after) = options.after {
        params["after"] = json!(after);
    }

    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_options() -> SearchOptions {
        SearchOptions {
            limit: 10,
            channel_types: vec![SearchChannelType::PublicChannel],
            content_types: vec![SearchContentType::Messages],
            context_channel_id: None,
            include_archived_channels: false,
            before: None,
            after: None,
            include_bots: false,
            disable_semantic_search: false,
            sort: SearchSort::Score,
            sort_dir: SearchSortDirection::Desc,
            include_context_messages: false,
            include_message_blocks: false,
            highlight: false,
        }
    }

    #[test]
    fn parses_real_time_search_message_response() {
        let response: SearchContextResponse = serde_json::from_value(json!({
            "ok": true,
            "results": {
                "messages": [
                    {
                        "author_name": "Jennifer Hynes",
                        "author_user_id": "U0123456",
                        "team_id": "T0123456",
                        "channel_id": "C0123456",
                        "channel_name": "proj-gizmo",
                        "message_ts": "123456.7890",
                        "content": "Project update",
                        "is_author_bot": false,
                        "permalink": "https://example.slack.com/archives/C0123456/p1234567890",
                        "blocks": []
                    }
                ],
                "files": [
                    {
                        "file_id": "F0123456",
                        "title": "Project tracker",
                        "file_type": "application/vnd.slack-list",
                        "permalink": "https://example.slack.com/lists/T0123456/F0123456"
                    }
                ],
                "channels": [
                    {
                        "name": "project-gizmo",
                        "topic": "Launch date",
                        "permalink": "https://slack.com/archives/C0123456"
                    }
                ],
                "users": [
                    {
                        "user_id": "U05KTJUUX5E",
                        "full_name": "Jason Chen",
                        "title": "Product Manager"
                    }
                ]
            },
            "response_metadata": {
                "next_cursor": "Q1VSUkVOVF9QQUdFOjI="
            }
        }))
        .expect("valid RTS response");

        assert_eq!(response.results.messages[0].ts, "123456.7890");
        assert_eq!(response.results.messages[0].text, "Project update");
        assert_eq!(
            response.results.files[0].file_id.as_deref(),
            Some("F0123456")
        );
        assert_eq!(
            response.results.channels[0].name.as_deref(),
            Some("project-gizmo")
        );
        assert_eq!(
            response.results.users[0].full_name.as_deref(),
            Some("Jason Chen")
        );
        assert_eq!(
            response.response_metadata.unwrap().next_cursor,
            "Q1VSUkVOVF9QQUdFOjI="
        );
    }

    #[test]
    fn truncates_combined_results_to_requested_limit() {
        let mut results = SearchResults {
            messages: vec![
                SearchMessageResult {
                    ts: "1.0".to_string(),
                    text: "one".to_string(),
                    team_id: None,
                    channel_id: None,
                    channel_name: None,
                    author_user_id: None,
                    author_name: None,
                    permalink: None,
                    is_author_bot: false,
                    blocks: None,
                    context_messages: None,
                },
                SearchMessageResult {
                    ts: "2.0".to_string(),
                    text: "two".to_string(),
                    team_id: None,
                    channel_id: None,
                    channel_name: None,
                    author_user_id: None,
                    author_name: None,
                    permalink: None,
                    is_author_bot: false,
                    blocks: None,
                    context_messages: None,
                },
            ],
            files: vec![SearchFileResult {
                file_id: Some("F1".to_string()),
                title: None,
                file_type: None,
                content: None,
                permalink: None,
                author_user_id: None,
                author_name: None,
                date_created: None,
                date_updated: None,
            }],
            channels: vec![],
            users: vec![],
        };

        results.truncate(2);

        assert_eq!(results.total_len(), 2);
        assert_eq!(results.messages.len(), 2);
        assert!(results.files.is_empty());
    }

    #[test]
    fn request_params_include_optional_filters() {
        let mut opts = sample_options();
        opts.before = Some(1_700_000_000);
        opts.after = Some(1_600_000_000);
        opts.context_channel_id = Some("C123ABCDE".to_string());
        opts.disable_semantic_search = true;
        opts.include_archived_channels = true;
        opts.highlight = true;
        opts.include_message_blocks = true;

        let params = build_request_params("hello", &opts);

        assert_eq!(params["query"], json!("hello"));
        assert_eq!(params["before"], json!(1_700_000_000));
        assert_eq!(params["after"], json!(1_600_000_000));
        assert_eq!(params["context_channel_id"], json!("C123ABCDE"));
        assert_eq!(params["disable_semantic_search"], json!(true));
        assert_eq!(params["include_archived_channels"], json!(true));
        assert_eq!(params["highlight"], json!(true));
        assert_eq!(params["include_message_blocks"], json!(true));
    }

    #[test]
    fn request_params_omit_unset_optional_filters() {
        let params = build_request_params("hello", &sample_options());
        assert!(params.get("before").is_none());
        assert!(params.get("after").is_none());
        assert!(params.get("context_channel_id").is_none());
    }
}
