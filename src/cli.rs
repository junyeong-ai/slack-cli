use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::auth::AuthMethod;
use crate::slack::{
    SearchChannelType, SearchContentType, SearchOptions, SearchSort, SearchSortDirection,
};

fn parse_search_limit(value: &str) -> Result<usize, String> {
    let max = SearchOptions::MAX_LIMIT;
    let limit = value
        .parse::<usize>()
        .map_err(|_| format!("limit must be an integer between 1 and {max}"))?;

    if (1..=max).contains(&limit) {
        Ok(limit)
    } else {
        Err(format!("limit must be between 1 and {max}"))
    }
}

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

    #[arg(
        long,
        short,
        global = true,
        help = "Path to config.toml (default: the platform config directory)"
    )]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, help = "Override the cache directory")]
    pub data_dir: Option<PathBuf>,

    #[arg(
        long,
        env = "SLACK_PROFILE",
        global = true,
        hide_env_values = true,
        help = "Select a stored auth profile for this invocation"
    )]
    pub profile: Option<String>,

    #[arg(long, short, global = true, help = "Emit machine-readable JSON output")]
    pub json: bool,

    #[arg(long, short, global = true, help = "Enable debug logging")]
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

        #[arg(
            long,
            value_delimiter = ',',
            value_name = "FIELDS",
            help = "Additional fields to include [id,name,real_name,display_name,email,status,status_emoji,avatar,title,timezone,is_admin,is_bot,deleted]"
        )]
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

        #[arg(
            long,
            value_delimiter = ',',
            value_name = "FIELDS",
            help = "Additional fields to include [id,name,type,members,topic,purpose,created,creator,is_member,is_archived,is_private,user]"
        )]
        expand: Option<Vec<String>>,
    },

    #[command(
        about = "Send a message to a channel or DM",
        long_about = "Send a message to a channel or DM.\n\
                      JSON sources for --blocks/--attachments/--metadata accept:\n  \
                        -          read from stdin (allowed for at most one flag)\n  \
                        @path.json read from file\n  \
                        <inline>   inline JSON literal"
    )]
    Send {
        channel: String,
        #[command(flatten)]
        content: MessageContent,
        #[arg(long, help = "Post as a reply in the given thread ts")]
        thread: Option<String>,
    },

    #[command(about = "Update a message")]
    Update {
        channel: String,
        ts: String,
        #[command(flatten)]
        content: MessageContent,
    },

    #[command(about = "Delete a message")]
    Delete { channel: String, ts: String },

    #[command(about = "Get the permalink URL for a message")]
    Permalink { channel: String, ts: String },

    #[command(about = "Get channel messages")]
    Messages {
        channel: String,
        #[arg(long, default_value = "15")]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, help = "Start time (Unix timestamp or ISO date: 2025-12-11)")]
        oldest: Option<String>,
        #[arg(long, help = "End time (Unix timestamp or ISO date: 2025-12-11)")]
        latest: Option<String>,
        #[arg(long, help = "Exclude bot messages")]
        exclude_bots: bool,
        #[arg(
            long,
            value_delimiter = ',',
            value_name = "FIELDS",
            help = "Additional fields beyond the lean default \
                    [blocks,attachments,reactions,edited,parent_user_id,reply_users,reply_users_count,latest_reply,channel,permalink,date,user_name]"
        )]
        expand: Option<Vec<String>>,
    },

    #[command(about = "Read thread messages")]
    Thread {
        channel: String,
        ts: String,
        #[arg(long, default_value = "15")]
        limit: usize,
        #[arg(long, help = "Exclude bot-authored replies")]
        exclude_bots: bool,
        #[arg(
            long,
            value_delimiter = ',',
            value_name = "FIELDS",
            help = "Additional fields beyond the lean default (same vocabulary as `messages --expand`)"
        )]
        expand: Option<Vec<String>>,
    },

    #[command(about = "List channel members")]
    Members { channel: String },

    #[command(about = "Search Slack context with Real-time Search API")]
    Search {
        #[arg(required_unless_present = "capabilities")]
        query: Option<String>,

        #[arg(
            long,
            conflicts_with = "query",
            help = "Report what this workspace supports instead of running a query"
        )]
        capabilities: bool,

        #[arg(
            long,
            default_value = "10",
            value_parser = parse_search_limit,
            help = "Maximum total results to return (1-100)"
        )]
        limit: usize,
        #[arg(
            long,
            value_enum,
            value_delimiter = ',',
            default_value = "public_channel,private_channel,mpim,im"
        )]
        channel_types: Vec<SearchChannelType>,
        #[arg(long, value_enum, value_delimiter = ',', default_value = "messages")]
        content_types: Vec<SearchContentType>,
        #[arg(long, help = "Restrict the search to one channel (ID or name)")]
        channel: Option<String>,
        #[arg(long, help = "Only results before this time (Unix ts or YYYY-MM-DD)")]
        before: Option<String>,
        #[arg(long, help = "Only results after this time (Unix ts or YYYY-MM-DD)")]
        after: Option<String>,
        #[arg(
            long = "include-context",
            help = "Include surrounding context messages"
        )]
        include_context_messages: bool,
        #[arg(long, help = "Include bot-authored messages")]
        include_bots: bool,
        #[arg(
            long = "include-deleted-users",
            help = "Include deleted users in results"
        )]
        include_deleted_users: bool,
        #[arg(long, help = "Search modifiers, e.g. \"has:pin from:@alice\"")]
        modifiers: Option<String>,
        #[arg(long = "include-archived", help = "Include archived channels")]
        include_archived_channels: bool,
        #[arg(long = "no-semantic", help = "Force keyword-only matching")]
        disable_semantic_search: bool,
        #[arg(long, value_enum, default_value = "score")]
        sort: SearchSort,
        #[arg(long, value_enum, default_value = "desc")]
        sort_dir: SearchSortDirection,
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

    #[command(
        about = "Stream matching Slack events to stdout as they happen",
        long_about = "Stream matching Slack events to stdout as they happen.\n\
                      Runs in the foreground and overrides both halves of the configured \n\
                      delivery: nothing is stored whatever `events.mode` says, and events go \n\
                      to stdout whatever sinks are configured. A consumer that is not reading \n\
                      misses what arrives. Use `daemon run` for the persistent form."
    )]
    Watch,

    #[command(about = "Socket Mode daemon")]
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    #[command(about = "Event log")]
    Events {
        #[command(subcommand)]
        action: EventsAction,
    },

    #[command(about = "Authentication management")]
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

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

    #[command(name = "self", about = "Manage this installation")]
    SelfCmd {
        #[command(subcommand)]
        action: SelfAction,
    },
}

#[derive(Subcommand)]
pub enum SelfAction {
    #[command(
        about = "Replace this binary with a published release",
        long_about = "Replace this binary with a published release.\n\
                      The download is checked against the release checksum, and \n\
                      against its sigstore signature when `cosign` is installed."
    )]
    Update {
        #[arg(
            long,
            value_name = "VER",
            help = "Install this version instead of the latest release"
        )]
        version: Option<String>,

        #[arg(long, help = "Report whether an update exists without installing it")]
        check: bool,

        #[arg(long, help = "Reinstall even when already at the target version")]
        force: bool,

        #[arg(long, short = 'y', help = "Do not prompt before replacing the binary")]
        yes: bool,
    },
}

// `MessageContent` is the content surface shared by `chat.postMessage` and
// `chat.update`. Adding a new payload field (e.g. `unfurl_links`, `mrkdwn`)
// belongs here, in `MessagePayload`, and in `main::build_payload` — never
// on individual command variants.
#[derive(Args, Debug, Clone)]
#[command(
    group(ArgGroup::new("message_content")
        .required(true)
        .multiple(true)
        .args(["text", "markdown_text", "blocks", "attachments"])),
)]
pub struct MessageContent {
    #[arg(
        long,
        short = 't',
        help = "Message text (also used as the notification fallback when blocks are present)"
    )]
    pub text: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["text", "blocks"],
        help = "Standard-markdown message body, rendered by Slack (no mrkdwn translation needed)"
    )]
    pub markdown_text: Option<String>,

    #[arg(
        long,
        short = 'b',
        help = "Block Kit blocks (JSON array): -, @path.json, or inline"
    )]
    pub blocks: Option<String>,

    #[arg(
        long,
        short = 'a',
        help = "Legacy attachments (JSON array): -, @path.json, or inline"
    )]
    pub attachments: Option<String>,

    #[arg(
        long,
        short = 'm',
        help = "Message metadata {event_type, event_payload} (JSON object): -, @path.json, or inline"
    )]
    pub metadata: Option<String>,
}

#[derive(Subcommand)]
pub enum AuthAction {
    #[command(
        about = "Authenticate to a Slack workspace",
        long_about = "Authenticate to a Slack workspace.\n\
                      Use the global --profile to name the saved profile (default: team slug)."
    )]
    Login {
        #[arg(long, value_enum, help = "Authentication method (default: pkce)")]
        method: Option<AuthMethodArg>,

        #[arg(long, help = "User token (xoxp-...) for static method")]
        user_token: Option<String>,

        #[arg(long, help = "Bot token (xoxb-...) for static method")]
        bot_token: Option<String>,

        #[arg(
            long,
            help = "App-level token (xapp-...) for Socket Mode; on its own it attaches to the resolved profile"
        )]
        app_token: Option<String>,

        #[arg(
            long,
            env = "SLACK_CLI_CLIENT_ID",
            hide_env_values = true,
            help = "OAuth client ID for the browser login"
        )]
        client_id: Option<String>,

        #[arg(
            long,
            value_parser = clap::value_parser!(u16).range(1..),
            help = "Loopback callback port for OAuth; must match a redirect URL registered on the app"
        )]
        port: Option<u16>,

        #[arg(long, help = "Do not open a browser; print the URL instead")]
        no_browser: bool,
    },

    #[command(
        about = "Remove a stored authentication profile",
        long_about = "Remove a stored authentication profile.\n\
                      Use the global --profile to target a specific profile (default: active)."
    )]
    Logout {
        #[arg(long, help = "Remove every stored profile")]
        all: bool,

        #[arg(long, help = "Skip the auth.revoke call to Slack")]
        keep_remote: bool,
    },

    #[command(
        about = "Show the active profile and verify the token",
        long_about = "Show profile details.\n\
                      Use the global --profile to inspect a specific profile (default: active)."
    )]
    Status {
        #[arg(long, help = "Hit auth.test to confirm the token still works")]
        verify: bool,
    },

    #[command(about = "List stored authentication profiles")]
    Profiles,

    #[command(about = "Switch the active profile")]
    Use { name: String },

    #[command(
        about = "Print the OAuth scopes this CLI needs",
        long_about = "Print the OAuth scopes this CLI needs.\n\
                      Derived from the Slack methods the CLI calls, so the list is \n\
                      always what `auth login` requests."
    )]
    Scopes,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum AuthMethodArg {
    Static,
    Pkce,
}

impl From<AuthMethodArg> for AuthMethod {
    fn from(value: AuthMethodArg) -> Self {
        match value {
            AuthMethodArg::Static => AuthMethod::Static,
            AuthMethodArg::Pkce => AuthMethod::Pkce,
        }
    }
}

#[derive(Subcommand)]
pub enum DaemonAction {
    #[command(
        about = "Run the Socket Mode daemon in the foreground",
        long_about = "Run the Socket Mode daemon in the foreground.\n\
                      It does not detach: run it under launchd, systemd or a supervisor, \n\
                      which restarts it and owns its logs. Slack splits an app's events \n\
                      across its open connections, so run exactly one per Slack app."
    )]
    Run,

    #[command(about = "Report what the running daemon last published")]
    Status,

    #[command(about = "Ask the running daemon to stop")]
    Stop,
}

#[derive(Subcommand)]
pub enum EventsAction {
    #[command(
        about = "Read events a consumer has not acknowledged yet",
        long_about = "Read events a consumer has not acknowledged yet.\n\
                      Each named consumer keeps its own position, so an agent that restarts \n\
                      resumes where it stopped. Needs events.mode = spool or archive."
    )]
    Pull {
        #[arg(
            long,
            default_value = "default",
            help = "Which position to read from; each name has its own"
        )]
        consumer: String,

        #[arg(
            long,
            default_value = "50",
            value_parser = clap::value_parser!(u16).range(1..),
            help = "Maximum events per batch"
        )]
        limit: u16,

        #[arg(
            long,
            help = "Acknowledge the batch as it is emitted (at-most-once; without it a crash re-reads)"
        )]
        ack: bool,

        #[arg(long, help = "Keep polling for new events instead of exiting")]
        follow: bool,
    },

    #[command(about = "Move a consumer's position forward")]
    Ack {
        #[arg(long, default_value = "default")]
        consumer: String,

        #[arg(long, help = "Acknowledge every event up to and including this seq")]
        through: i64,
    },

    #[command(about = "Show the event log's size, age and consumer backlogs")]
    Stats,

    #[command(about = "Apply the retention policy now")]
    Prune,

    #[command(about = "Show where the daemon keeps its files")]
    Path,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    #[command(about = "Show current configuration")]
    Show,

    #[command(about = "Show configuration file path")]
    Path,

    #[command(about = "Edit configuration with the default editor")]
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
