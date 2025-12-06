use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "slack-cli",
    version,
    about = "Slack CLI with FTS5 cache",
    author
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[arg(long, env = "SLACK_BOT_TOKEN", global = true, hide_env_values = true)]
    pub token: Option<String>,

    #[arg(long, env = "SLACK_USER_TOKEN", global = true, hide_env_values = true)]
    pub user_token: Option<String>,

    #[arg(long, short, global = true)]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,

    #[arg(long, short, global = true)]
    pub json: bool,

    #[arg(long, short, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Search users by name/email or get by IDs")]
    Users {
        #[arg(required_unless_present = "id")]
        query: Option<String>,

        #[arg(long, value_delimiter = ',', conflicts_with = "query")]
        id: Option<Vec<String>>,

        #[arg(long, default_value = "10")]
        limit: usize,

        #[arg(long, value_delimiter = ',', value_name = "FIELDS",
              help = "Additional fields to include [id,name,real_name,display_name,email,status,status_emoji,avatar,title,timezone,is_admin,is_bot,deleted]")]
        expand: Option<Vec<String>>,
    },

    #[command(about = "Search channels by name or get by IDs")]
    Channels {
        #[arg(required_unless_present = "id")]
        query: Option<String>,

        #[arg(long, value_delimiter = ',', conflicts_with = "query")]
        id: Option<Vec<String>>,

        #[arg(long, default_value = "10")]
        limit: usize,

        #[arg(long, value_delimiter = ',', value_name = "FIELDS",
              help = "Additional fields to include [id,name,type,members,topic,purpose,created,creator,is_member,is_archived,is_private]")]
        expand: Option<Vec<String>>,
    },

    #[command(about = "Send message to channel or DM")]
    Send {
        channel: String,
        text: String,
        #[arg(long)]
        thread: Option<String>,
    },

    #[command(about = "Update a message")]
    Update {
        channel: String,
        ts: String,
        text: String,
    },

    #[command(about = "Delete a message")]
    Delete { channel: String, ts: String },

    #[command(about = "Get channel messages")]
    Messages {
        channel: String,
        #[arg(long, default_value = "100")]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
    },

    #[command(about = "Read thread messages")]
    Thread {
        channel: String,
        ts: String,
        #[arg(long, default_value = "100")]
        limit: usize,
    },

    #[command(about = "List channel members")]
    Members { channel: String },

    #[command(about = "Search messages (requires user token)")]
    Search {
        query: String,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    #[command(about = "Add reaction to a message")]
    React {
        channel: String,
        ts: String,
        emoji: String,
    },

    #[command(about = "Remove reaction from a message")]
    Unreact {
        channel: String,
        ts: String,
        emoji: String,
    },

    #[command(about = "Get reactions on a message")]
    Reactions { channel: String, ts: String },

    #[command(about = "List custom emoji")]
    Emoji {
        #[arg(long)]
        query: Option<String>,
    },

    #[command(about = "Pin a message")]
    Pin { channel: String, ts: String },

    #[command(about = "Unpin a message")]
    Unpin { channel: String, ts: String },

    #[command(about = "List pinned messages")]
    Pins { channel: String },

    #[command(about = "Add a bookmark")]
    Bookmark {
        channel: String,
        title: String,
        url: String,
        #[arg(long)]
        emoji: Option<String>,
    },

    #[command(about = "Remove a bookmark")]
    Unbookmark {
        channel: String,
        bookmark_id: String,
    },

    #[command(about = "List bookmarks")]
    Bookmarks { channel: String },

    #[command(about = "Configuration management")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    #[command(about = "Cache management")]
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    #[command(about = "Initialize configuration")]
    Init {
        #[arg(long)]
        bot_token: Option<String>,
        #[arg(long)]
        user_token: Option<String>,
        #[arg(long)]
        force: bool,
    },

    #[command(about = "Show current configuration (tokens masked)")]
    Show,

    #[command(about = "Show configuration file path")]
    Path,

    #[command(about = "Edit configuration with default editor")]
    Edit,
}

#[derive(Subcommand)]
pub enum CacheAction {
    #[command(about = "Refresh cache data")]
    Refresh {
        #[arg(value_enum, default_value = "all")]
        target: RefreshTarget,
    },

    #[command(about = "Show cache statistics")]
    Stats,

    #[command(about = "Show cache file path")]
    Path,
}

#[derive(ValueEnum, Clone)]
pub enum RefreshTarget {
    Users,
    Channels,
    All,
}
