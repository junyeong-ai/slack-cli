use crate::cache::SqliteCache;
use crate::events::{DaemonStatus, Event, PruneOutcome, StoreCaps, StoreStats};
use crate::slack::types::{MessageFile, SlackChannel, SlackMessage, SlackUser};
use crate::slack::{
    Bookmark, CustomEmoji, MessageReactions, PinnedMessage, ScheduleResponse, ScheduledMessage,
    SearchCapabilities, SearchResults,
};
use crate::update::install::Signature;
use crate::update::{Action, UpdateOutcome};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::io::{IsTerminal, Write};

/// A field `users` can print. `--expand` and `[output].users_fields` share
/// this vocabulary, so a misspelled name is refused instead of printing nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum UserField {
    Id,
    Name,
    RealName,
    DisplayName,
    Email,
    Status,
    StatusEmoji,
    Avatar,
    Title,
    Timezone,
    IsAdmin,
    IsBot,
    Deleted,
}

impl UserField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::RealName => "real_name",
            Self::DisplayName => "display_name",
            Self::Email => "email",
            Self::Status => "status",
            Self::StatusEmoji => "status_emoji",
            Self::Avatar => "avatar",
            Self::Title => "title",
            Self::Timezone => "timezone",
            Self::IsAdmin => "is_admin",
            Self::IsBot => "is_bot",
            Self::Deleted => "deleted",
        }
    }
}

/// A field `channels` can print; shared by `--expand` and
/// `[output].channels_fields`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum ChannelField {
    Id,
    Name,
    User,
    Type,
    Members,
    Topic,
    Purpose,
    Created,
    Creator,
    IsMember,
    IsArchived,
    IsPrivate,
}

impl ChannelField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::User => "user",
            Self::Type => "type",
            Self::Members => "members",
            Self::Topic => "topic",
            Self::Purpose => "purpose",
            Self::Created => "created",
            Self::Creator => "creator",
            Self::IsMember => "is_member",
            Self::IsArchived => "is_archived",
            Self::IsPrivate => "is_private",
        }
    }
}

/// A field `messages` and `thread` can print; shared by `--expand` and
/// `[output].messages_fields`. Every name except the computed `date` and
/// `user_name` is a key of the serialized `SlackMessage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum MessageField {
    Ts,
    User,
    BotId,
    Username,
    Text,
    Channel,
    ThreadTs,
    ReplyCount,
    ReplyUsers,
    ReplyUsersCount,
    LatestReply,
    ParentUserId,
    Reactions,
    Subtype,
    Edited,
    Blocks,
    Attachments,
    Files,
    Permalink,
    Metadata,
    Date,
    UserName,
}

impl MessageField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::User => "user",
            Self::BotId => "bot_id",
            Self::Username => "username",
            Self::Text => "text",
            Self::Channel => "channel",
            Self::ThreadTs => "thread_ts",
            Self::ReplyCount => "reply_count",
            Self::ReplyUsers => "reply_users",
            Self::ReplyUsersCount => "reply_users_count",
            Self::LatestReply => "latest_reply",
            Self::ParentUserId => "parent_user_id",
            Self::Reactions => "reactions",
            Self::Subtype => "subtype",
            Self::Edited => "edited",
            Self::Blocks => "blocks",
            Self::Attachments => "attachments",
            Self::Files => "files",
            Self::Permalink => "permalink",
            Self::Metadata => "metadata",
            Self::Date => "date",
            Self::UserName => "user_name",
        }
    }
}

/// Why a command's output was not written. `Closed` is the reader leaving
/// early, as `| head` does once it has what it wants; `main` exits 0 on it
/// without a message.
#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("standard output was closed")]
    Closed,
    #[error("failed to write to standard output")]
    Write(#[source] std::io::Error),
    #[error("failed to serialize output")]
    Serialize(#[from] serde_json::Error),
}

impl From<std::io::Error> for OutputError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            Self::Closed
        } else {
            Self::Write(err)
        }
    }
}

/// `println!` for command output, returning an `OutputError` where
/// `println!` would panic.
#[macro_export]
macro_rules! outln {
    ($($arg:tt)*) => {
        $crate::format::write_line(::std::format_args!($($arg)*))
    };
}

#[doc(hidden)]
pub fn write_line(line: std::fmt::Arguments<'_>) -> Result<(), OutputError> {
    writeln!(std::io::stdout().lock(), "{line}")?;
    Ok(())
}

/// `eprintln!` for diagnostics, without the panic: once stderr is gone there
/// is nowhere left to report to, so a failed write is dropped and the command
/// keeps its exit status.
#[macro_export]
macro_rules! errln {
    ($($arg:tt)*) => {
        $crate::format::write_diagnostic(::std::format_args!($($arg)*))
    };
}

#[doc(hidden)]
pub fn write_diagnostic(line: std::fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

/// Writes one JSON document to stdout: indented for a terminal, a single line
/// for a pipe, which is how a program or an agent reads it.
pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<(), OutputError> {
    let json = if std::io::stdout().is_terminal() {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    outln!("{json}")
}

pub fn print_path(path: &std::path::Path, as_json: bool) -> Result<(), OutputError> {
    if as_json {
        print_json(&json!({ "path": path.display().to_string() }))?;
    } else {
        outln!("{}", path.display())?;
    }
    Ok(())
}

pub fn print_users(
    users: &[SlackUser],
    fields: &[UserField],
    as_json: bool,
) -> Result<(), OutputError> {
    if as_json {
        let rows: Vec<Value> = users
            .iter()
            .map(|user| {
                let row: Map<String, Value> = fields
                    .iter()
                    .map(|&field| (field.as_str().to_owned(), user_value(user, field)))
                    .collect();
                Value::Object(row)
            })
            .collect();
        return print_json(&rows);
    }

    if users.is_empty() {
        outln!("No users found")?;
        return Ok(());
    }

    for user in users {
        let cells: Vec<String> = fields.iter().map(|&field| user_cell(user, field)).collect();
        outln!("{}", cells.join("\t"))?;
    }
    Ok(())
}

fn user_status(user: &SlackUser) -> Option<String> {
    let profile = user.profile.as_ref()?;
    match (profile.status_text.as_ref(), profile.status_emoji.as_ref()) {
        (Some(t), Some(e)) if !t.is_empty() => Some(format!("{} {}", e, t)),
        (Some(t), _) if !t.is_empty() => Some(t.clone()),
        (_, Some(e)) if !e.is_empty() => Some(e.clone()),
        _ => None,
    }
}

fn user_value(user: &SlackUser, field: UserField) -> Value {
    let profile = user.profile.as_ref();
    match field {
        UserField::Id => json!(user.id),
        UserField::Name => json!(user.name),
        UserField::RealName => json!(profile.and_then(|p| p.real_name.as_ref())),
        UserField::DisplayName => json!(profile.and_then(|p| p.display_name.as_ref())),
        UserField::Email => json!(profile.and_then(|p| p.email.as_ref())),
        UserField::Status => json!(user_status(user).unwrap_or_default()),
        UserField::StatusEmoji => json!(profile.and_then(|p| p.status_emoji.as_ref())),
        UserField::Avatar => json!(profile.and_then(|p| p.avatar.as_ref())),
        UserField::Title => json!(profile.and_then(|p| p.title.as_ref())),
        UserField::Timezone => json!(profile.and_then(|p| p.timezone.as_ref())),
        UserField::IsAdmin => json!(user.is_admin),
        UserField::IsBot => json!(user.is_bot),
        UserField::Deleted => json!(user.deleted),
    }
}

fn user_cell(user: &SlackUser, field: UserField) -> String {
    let profile = user.profile.as_ref();
    let text = |value: Option<&String>| value.cloned().unwrap_or_else(|| "-".to_string());
    let non_empty = |value: Option<&String>| {
        value
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "-".to_string())
    };
    match field {
        UserField::Id => user.id.clone(),
        UserField::Name => user.name.clone(),
        UserField::RealName => text(profile.and_then(|p| p.real_name.as_ref())),
        UserField::DisplayName => non_empty(profile.and_then(|p| p.display_name.as_ref())),
        UserField::Email => text(profile.and_then(|p| p.email.as_ref())),
        UserField::Status => user_status(user).unwrap_or_else(|| "-".to_string()),
        UserField::StatusEmoji => non_empty(profile.and_then(|p| p.status_emoji.as_ref())),
        UserField::Avatar => text(profile.and_then(|p| p.avatar.as_ref())),
        UserField::Title => non_empty(profile.and_then(|p| p.title.as_ref())),
        UserField::Timezone => text(profile.and_then(|p| p.timezone.as_ref())),
        UserField::IsAdmin => if user.is_admin { "admin" } else { "-" }.to_string(),
        UserField::IsBot => if user.is_bot { "bot" } else { "-" }.to_string(),
        UserField::Deleted => if user.deleted { "deleted" } else { "-" }.to_string(),
    }
}

pub fn print_channels(
    channels: &[SlackChannel],
    fields: &[ChannelField],
    as_json: bool,
) -> Result<(), OutputError> {
    if as_json {
        let rows: Vec<Value> = channels
            .iter()
            .map(|channel| {
                let row: Map<String, Value> = fields
                    .iter()
                    .map(|&field| (field.as_str().to_owned(), channel_value(channel, field)))
                    .collect();
                Value::Object(row)
            })
            .collect();
        return print_json(&rows);
    }

    if channels.is_empty() {
        outln!("No channels found")?;
        return Ok(());
    }

    for channel in channels {
        let cells: Vec<String> = fields
            .iter()
            .map(|&field| channel_cell(channel, field))
            .collect();
        outln!("{}", cells.join("\t"))?;
    }
    Ok(())
}

fn channel_type(ch: &SlackChannel) -> &'static str {
    if ch.is_im {
        "DM"
    } else if ch.is_mpim {
        "Group"
    } else if ch.is_private {
        "Private"
    } else {
        "Public"
    }
}

fn channel_value(ch: &SlackChannel, field: ChannelField) -> Value {
    match field {
        ChannelField::Id => json!(ch.id),
        ChannelField::Name => json!(ch.name),
        ChannelField::User => json!(ch.user),
        ChannelField::Type => json!(channel_type(ch)),
        ChannelField::Members => json!(ch.num_members),
        ChannelField::Topic => json!(ch.topic.as_ref().map(|t| &t.value)),
        ChannelField::Purpose => json!(ch.purpose.as_ref().map(|p| &p.value)),
        ChannelField::Created => json!(ch.created),
        ChannelField::Creator => json!(ch.creator),
        ChannelField::IsMember => json!(ch.is_member),
        ChannelField::IsArchived => json!(ch.is_archived),
        ChannelField::IsPrivate => json!(ch.is_private),
    }
}

fn channel_cell(ch: &SlackChannel, field: ChannelField) -> String {
    let text = |value: Option<&String>| value.cloned().unwrap_or_else(|| "-".to_string());
    let non_empty = |value: Option<&String>| {
        value
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "-".to_string())
    };
    match field {
        ChannelField::Id => ch.id.clone(),
        ChannelField::Name => text(ch.name.as_ref()),
        ChannelField::User => text(ch.user.as_ref()),
        ChannelField::Type => channel_type(ch).to_string(),
        ChannelField::Members => ch
            .num_members
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string()),
        ChannelField::Topic => non_empty(ch.topic.as_ref().map(|t| &t.value)),
        ChannelField::Purpose => non_empty(ch.purpose.as_ref().map(|p| &p.value)),
        ChannelField::Created => ch
            .created
            .map(|ts| {
                DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| ts.to_string())
            })
            .unwrap_or_else(|| "-".to_string()),
        ChannelField::Creator => text(ch.creator.as_ref()),
        ChannelField::IsMember => if ch.is_member { "member" } else { "-" }.to_string(),
        ChannelField::IsArchived => if ch.is_archived { "archived" } else { "-" }.to_string(),
        ChannelField::IsPrivate => if ch.is_private { "private" } else { "public" }.to_string(),
    }
}

/// One page of a channel's history or a thread plus the cursor to the next
/// page: `{messages, next_cursor}` with a null cursor on the last page, so a
/// reader can tell a complete conversation from a truncated one.
pub fn print_message_page(
    messages: &[SlackMessage],
    next_cursor: Option<&str>,
    as_json: bool,
    fields: &[MessageField],
    cache: Option<&SqliteCache>,
) -> Result<(), OutputError> {
    let allowed = field_names(fields);

    if as_json {
        let envelope = json!({
            "messages": project_messages(messages, &allowed, cache)?,
            "next_cursor": next_cursor,
        });
        return print_json(&envelope);
    }

    render_messages(messages, &allowed, cache)?;
    if let Some(cursor) = next_cursor {
        errln!("More messages available: rerun with --cursor {}", cursor);
    }
    Ok(())
}

fn field_names(fields: &[MessageField]) -> HashSet<&'static str> {
    fields.iter().map(|field| field.as_str()).collect()
}

fn project_messages(
    messages: &[SlackMessage],
    allowed: &HashSet<&str>,
    cache: Option<&SqliteCache>,
) -> Result<Vec<Value>, serde_json::Error> {
    messages
        .iter()
        .map(|msg| project_message(msg, allowed, cache))
        .collect()
}

fn render_messages(
    messages: &[SlackMessage],
    allowed: &HashSet<&str>,
    cache: Option<&SqliteCache>,
) -> Result<(), OutputError> {
    if messages.is_empty() {
        outln!("No messages found")?;
        return Ok(());
    }

    let expand_date = allowed.contains(MessageField::Date.as_str());
    let expand_user_name = allowed.contains(MessageField::UserName.as_str());

    for msg in messages {
        // Priority: user > username (bot display name) > bot_id > "system"
        let author_id = msg
            .user
            .as_deref()
            .or(msg.username.as_deref())
            .or(msg.bot_id.as_deref())
            .unwrap_or("system");

        let author = if expand_user_name {
            msg.user
                .as_ref()
                .and_then(|id| resolve_user_name(id, cache))
                .unwrap_or_else(|| author_id.to_string())
        } else {
            author_id.to_string()
        };

        let ts_display = if expand_date {
            format_timestamp(&msg.ts).unwrap_or_else(|| msg.ts.clone())
        } else {
            msg.ts.clone()
        };

        outln!("[{}] {}: {}", ts_display, author, msg.text)?;

        // Render attachments (wee-slack style)
        if let Some(attachments) = &msg.attachments {
            for att in attachments {
                render_attachment(att)?;
            }
        }
        for file in msg.files.iter().flatten() {
            render_file(file)?;
        }

        if let Some(count) = msg.reply_count {
            outln!("  └─ {} replies", count)?;
        }
    }
    Ok(())
}

fn project_message(
    msg: &SlackMessage,
    allowed: &HashSet<&str>,
    cache: Option<&SqliteCache>,
) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(msg)?;
    if let Value::Object(map) = &mut value {
        map.retain(|key, _| allowed.contains(key.as_str()));

        if allowed.contains(MessageField::Date.as_str())
            && let Some(date_str) = format_timestamp(&msg.ts)
        {
            map.insert("date".to_string(), json!(date_str));
        }
        if allowed.contains(MessageField::UserName.as_str())
            && let Some(user_id) = &msg.user
            && let Some(name) = resolve_user_name(user_id, cache)
        {
            map.insert("user_name".to_string(), json!(name));
        }
    }
    Ok(value)
}

fn format_timestamp(ts: &str) -> Option<String> {
    let ts_secs: i64 = ts.split('.').next()?.parse().ok()?;
    DateTime::from_timestamp(ts_secs, 0).map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn resolve_user_name(user_id: &str, cache: Option<&SqliteCache>) -> Option<String> {
    cache?
        .get_user_by_id(user_id)
        .ok()
        .flatten()
        .and_then(|u| u.profile.and_then(|p| p.real_name))
}

fn render_file(file: &MessageFile) -> Result<(), OutputError> {
    let label = file
        .title
        .as_deref()
        .or(file.name.as_deref())
        .unwrap_or(&file.id);
    match file.mimetype.as_deref().or(file.mode.as_deref()) {
        Some(kind) => outln!("  [file] {label} ({kind})")?,
        None => outln!("  [file] {label}")?,
    }
    Ok(())
}

fn render_attachment(att: &Value) -> Result<(), OutputError> {
    let mut rendered = false;

    if let Some(pretext) = att.get("pretext").and_then(|v| v.as_str())
        && !pretext.is_empty()
    {
        outln!("  │ {}", pretext)?;
        rendered = true;
    }

    let author = att.get("author_name").and_then(|v| v.as_str());
    let title = att.get("title").and_then(|v| v.as_str());
    match (author, title) {
        (Some(a), Some(t)) => {
            outln!("  │ {}: {}", a, t)?;
            rendered = true;
        }
        (Some(a), None) => {
            outln!("  │ {}", a)?;
            rendered = true;
        }
        (None, Some(t)) => {
            outln!("  │ {}", t)?;
            rendered = true;
        }
        _ => {}
    }

    if let Some(text) = att.get("text").and_then(|v| v.as_str())
        && !text.is_empty()
    {
        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                outln!("  │ {}", trimmed)?;
            }
        }
        rendered = true;
    }

    if let Some(fields) = att.get("fields").and_then(|v| v.as_array()) {
        for field in fields {
            let field_title = field.get("title").and_then(|v| v.as_str());
            let field_value = field.get("value").and_then(|v| v.as_str());
            match (field_title, field_value) {
                (Some(t), Some(v)) => {
                    let first_line = v.lines().next().unwrap_or(v);
                    outln!("  │ {}: {}", t, first_line)?;
                    rendered = true;
                }
                (None, Some(v)) => {
                    let first_line = v.lines().next().unwrap_or(v);
                    outln!("  │ {}", first_line)?;
                    rendered = true;
                }
                _ => {}
            }
        }
    }

    if let Some(footer) = att.get("footer").and_then(|v| v.as_str())
        && !footer.is_empty()
    {
        outln!("  │ {}", footer)?;
        rendered = true;
    }

    if !rendered
        && let Some(fallback) = att.get("fallback").and_then(|v| v.as_str())
        && !fallback.is_empty()
    {
        outln!("  │ {}", fallback)?;
    }
    Ok(())
}

pub fn print_members(
    member_ids: &[String],
    cache: &crate::cache::SqliteCache,
    as_json: bool,
) -> Result<(), OutputError> {
    if as_json {
        return print_json(member_ids);
    }

    if member_ids.is_empty() {
        outln!("No members found")?;
        return Ok(());
    }

    let users = cache.get_users().unwrap_or_default();

    for id in member_ids {
        if let Some(user) = users.iter().find(|u| &u.id == id) {
            outln!("{:<20} {}", user.name, id)?;
        } else {
            outln!("{}", id)?;
        }
    }
    Ok(())
}

pub fn print_reactions(reactions: &MessageReactions, as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(reactions);
    }

    if reactions.reactions.is_empty() {
        outln!("No reactions")?;
        return Ok(());
    }

    for r in &reactions.reactions {
        outln!(":{}: ({})", r.name, r.count)?;
    }
    Ok(())
}

pub fn print_emoji(emoji: &[CustomEmoji], as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(emoji);
    }

    if emoji.is_empty() {
        outln!("No custom emoji found")?;
        return Ok(());
    }

    for e in emoji {
        if e.is_alias {
            outln!(
                ":{}: -> :{}: (alias)",
                e.name,
                e.alias_for.as_deref().unwrap_or("?")
            )?;
        } else {
            outln!(":{}: {}", e.name, e.url)?;
        }
    }
    Ok(())
}

pub fn print_pins(pins: &[PinnedMessage], as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(pins);
    }

    if pins.is_empty() {
        outln!("No pinned messages")?;
        return Ok(());
    }

    for pin in pins {
        let text = pin.text.as_deref().unwrap_or("[no text]");
        let preview: String = text.chars().take(60).collect();
        if text.chars().count() > 60 {
            outln!("[{}] {}...", pin.ts, preview)?;
        } else {
            outln!("[{}] {}", pin.ts, preview)?;
        }
    }
    Ok(())
}

pub fn print_schedule(scheduled: &ScheduleResponse, as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(scheduled);
    }

    outln!(
        "✓ Scheduled for {} (id: {})",
        format_epoch(scheduled.post_at),
        scheduled.scheduled_message_id
    )
}

pub fn print_scheduled(messages: &[ScheduledMessage], as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(messages);
    }

    if messages.is_empty() {
        outln!("No scheduled messages")?;
        return Ok(());
    }

    for message in messages {
        let preview: String = message.text.chars().take(60).collect();
        let ellipsis = if message.text.chars().count() > 60 {
            "..."
        } else {
            ""
        };
        outln!(
            "{}  {}  {}  {}{}",
            format_epoch(message.post_at),
            message.channel_id,
            message.id,
            preview,
            ellipsis
        )?;
    }
    Ok(())
}

pub fn print_bookmarks(bookmarks: &[Bookmark], as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(bookmarks);
    }

    if bookmarks.is_empty() {
        outln!("No bookmarks")?;
        return Ok(());
    }

    for b in bookmarks {
        let emoji = b.emoji.as_deref().unwrap_or("");
        outln!("{} {} - {} (id: {})", emoji, b.title, b.link, b.id)?;
    }
    Ok(())
}

pub fn print_search_capabilities(
    capabilities: &SearchCapabilities,
    as_json: bool,
) -> Result<(), OutputError> {
    if as_json {
        return print_json(&json!({ "is_ai_search_enabled": capabilities.is_ai_search_enabled }));
    }

    outln!(
        "semantic search: {}",
        if capabilities.is_ai_search_enabled {
            "enabled"
        } else {
            "unavailable (keyword matching only)"
        }
    )
}

pub fn print_search_results(results: &SearchResults, as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(results);
    }

    if results.messages.is_empty()
        && results.files.is_empty()
        && results.channels.is_empty()
        && results.users.is_empty()
    {
        outln!("No search results found")?;
        return Ok(());
    }

    for msg in &results.messages {
        let author = msg
            .author_name
            .as_deref()
            .or(msg.author_user_id.as_deref())
            .unwrap_or("unknown");
        let channel = msg.channel_name.as_deref().unwrap_or("-");
        outln!("[message] #{} {}: {}", channel, author, msg.text)?;
        if let Some(permalink) = &msg.permalink {
            outln!("  {}", permalink)?;
        }
    }

    for file in &results.files {
        let title = file.title.as_deref().unwrap_or("[untitled file]");
        let file_type = file.file_type.as_deref().unwrap_or("file");
        outln!("[file] {} ({})", title, file_type)?;
        if let Some(permalink) = &file.permalink {
            outln!("  {}", permalink)?;
        }
    }

    for channel in &results.channels {
        let name = channel.name.as_deref().unwrap_or("[unnamed channel]");
        outln!("[channel] #{}", name)?;
        if let Some(topic) = channel.topic.as_deref().filter(|topic| !topic.is_empty()) {
            outln!("  {}", topic)?;
        }
        if let Some(permalink) = &channel.permalink {
            outln!("  {}", permalink)?;
        }
    }

    for user in &results.users {
        let name = user
            .full_name
            .as_deref()
            .or(user.user_id.as_deref())
            .unwrap_or("[unknown user]");
        outln!("[user] {}", name)?;
        if let Some(title) = user.title.as_deref().filter(|title| !title.is_empty()) {
            outln!("  {}", title)?;
        }
        if let Some(permalink) = &user.permalink {
            outln!("  {}", permalink)?;
        }
    }
    Ok(())
}

pub fn print_update_outcome(outcome: &UpdateOutcome, as_json: bool) -> Result<(), OutputError> {
    let signature = outcome.signature.map(|signature| match signature {
        Signature::Verified => "verified",
        Signature::Unverified => "unverified",
    });

    if as_json {
        return print_json(&json!({
            "action": outcome.action.as_str(),
            "from": outcome.from,
            "to": outcome.to,
            "target": outcome.target,
            "binary": outcome.binary.display().to_string(),
            "signature": signature,
        }));
    }

    match outcome.action {
        Action::AlreadyCurrent => outln!("Already at v{} ({})", outcome.to, outcome.target)?,
        Action::UpdateAvailable => outln!(
            "v{} is available (running v{}). Run: slack-cli self update",
            outcome.to,
            outcome.from
        )?,
        Action::Cancelled => outln!("Cancelled; still at v{}", outcome.from)?,
        Action::Updated | Action::Reinstalled => {
            outln!(
                "✓ {} to v{} at {}",
                if outcome.action == Action::Reinstalled {
                    "Reinstalled"
                } else {
                    "Updated"
                },
                outcome.to,
                outcome.binary.display()
            )?;
            if signature == Some("unverified") {
                outln!(
                    "  cosign is not installed, so the signature was not checked and the \
                     download rests on its checksum alone."
                )?;
            }
        }
    }
    Ok(())
}

/// One JSON object per line, which is what a consuming process reads. Not
/// `print_json`, which indents for a terminal: a pulled batch is a stream of
/// records, and a record that spans lines cannot be read one at a time.
pub fn print_events(events: &[Event], as_json: bool) -> Result<(), OutputError> {
    if as_json {
        for event in events {
            outln!("{}", event.to_ndjson()?)?;
        }
        return Ok(());
    }

    if events.is_empty() {
        outln!("No events")?;
        return Ok(());
    }

    for event in events {
        outln!(
            "{:>6}  {}  [{}]  {}  {}  {}",
            event.seq,
            event.received_at.format("%Y-%m-%d %H:%M:%S"),
            event.matched.join(","),
            event.kind.as_str(),
            event.channel.as_deref().unwrap_or("-"),
            event
                .text
                .as_deref()
                .map(|text| {
                    let single = text.replace('\n', " ");
                    let mut cut: String = single.chars().take(80).collect();
                    if single.chars().count() > 80 {
                        cut.push('…');
                    }
                    cut
                })
                .unwrap_or_else(|| "(body not stored)".to_string()),
        )?;
    }
    Ok(())
}

pub fn print_event_stats(
    stats: &StoreStats,
    caps: &StoreCaps,
    mode: &str,
    profile: &str,
    as_json: bool,
) -> Result<(), OutputError> {
    if as_json {
        return print_json(&json!({
            "profile": profile,
            "mode": mode,
            "durable": caps.durable,
            "replayable": caps.replayable,
            "events": stats.events,
            "bytes": stats.bytes,
            "oldest": stats.oldest,
            "newest": stats.newest,
            "consumers": stats.consumers,
        }));
    }

    outln!("profile : {profile}")?;
    outln!("mode    : {mode}")?;
    if !caps.durable {
        outln!("events  : not stored (streaming only)")?;
        return Ok(());
    }

    outln!("events  : {}", stats.events)?;
    outln!("size    : {}", human_bytes(stats.bytes))?;
    if let (Some(oldest), Some(newest)) = (stats.oldest, stats.newest) {
        outln!("oldest  : {}", format_epoch(oldest))?;
        outln!("newest  : {}", format_epoch(newest))?;
    }
    if stats.consumers.is_empty() {
        outln!("consumers: none registered")?;
    } else {
        outln!("consumers:")?;
        for consumer in &stats.consumers {
            outln!(
                "  {:<20} acked {:<10} pending {}",
                consumer.name,
                consumer.acked_seq,
                consumer.pending
            )?;
        }
    }
    Ok(())
}

/// `running` answers whether a daemon process is actually alive; only the
/// daemon lock can say so. The heartbeat cannot: a daemon killed a moment ago
/// leaves a fresh one behind, so reading liveness from its age reports a dead
/// daemon as running until the record goes stale. `stale_after` is what
/// separates a live daemon from one that still holds the lock but has stopped
/// heartbeating — wedged rather than gone.
pub fn print_daemon_status(
    status: Option<&DaemonStatus>,
    profile: &str,
    running: bool,
    stale_after: i64,
    as_json: bool,
) -> Result<(), OutputError> {
    // Named on every line of output, because the events of one installation
    // live apart from another's: a command run without the environment the
    // daemon runs under reads a different store, finds it empty, and would
    // otherwise report a working daemon as absent.
    let Some(status) = status else {
        if as_json {
            print_json(&json!({ "running": false, "profile": profile }))?;
        } else {
            outln!("profile : {profile}")?;
            outln!(
                "running : no daemon has run for this profile. Start one: slack-cli daemon run"
            )?;
        }
        return Ok(());
    };

    let age = chrono::Utc::now().timestamp() - status.heartbeat_at;
    let beating = age <= stale_after;

    if as_json {
        return print_json(&json!({
            "running": running,
            "profile": profile,
            "pid": status.pid,
            "connected": status.connected && running && beating,
            "started_at": status.started_at,
            "heartbeat_at": status.heartbeat_at,
            "heartbeat_age_seconds": age,
            "counters": {
                "received": status.counters.received,
                "matched": status.counters.matched,
                "stored": status.counters.stored,
                "dropped": status.counters.dropped,
                "delivered": status.counters.delivered,
                "failed": status.counters.failed,
                "reconnects": status.counters.reconnects,
                "backfilled": status.counters.backfilled,
            },
        }));
    }

    outln!("profile  : {profile}")?;
    if !running {
        outln!(
            "stopped  : pid {} ran from {} until its last heartbeat {}s ago",
            status.pid,
            format_epoch(status.started_at),
            age
        )?;
    } else if beating {
        outln!(
            "running  : pid {} ({}), {}",
            status.pid,
            if status.connected {
                "connected"
            } else {
                "reconnecting"
            },
            format_epoch(status.started_at)
        )?;
    } else {
        outln!(
            "wedged   : pid {} still holds the lock but has not heartbeat for {}s (running \
             since {}). Stop it: slack-cli daemon stop",
            status.pid,
            age,
            format_epoch(status.started_at)
        )?;
    }

    let counters = &status.counters;
    outln!(
        "events   : {} received, {} matched, {} stored, {} recovered",
        counters.received,
        counters.matched,
        counters.stored,
        counters.backfilled
    )?;
    outln!(
        "delivery : {} delivered, {} failed, {} reconnects",
        counters.delivered,
        counters.failed,
        counters.reconnects
    )?;
    if counters.dropped > 0 {
        outln!(
            "dropped  : {} — the buffer overflowed. Raise events.buffer, or make the sink \
             faster: delivery is serial, so one slow handler holds the whole pipeline",
            counters.dropped
        )?;
    }
    Ok(())
}

pub fn print_prune_outcome(outcome: &PruneOutcome, as_json: bool) -> Result<(), OutputError> {
    if as_json {
        return print_json(&json!({
            "removed": outcome.total(),
            "acknowledged": outcome.acknowledged,
            "expired": outcome.expired,
            "over_budget": outcome.over_budget,
        }));
    }

    outln!(
        "✓ Removed {} events ({} acknowledged, {} expired, {} over the size budget)",
        outcome.total(),
        outcome.acknowledged,
        outcome.expired,
        outcome.over_budget
    )
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A stored epoch as a local timestamp. Public because `main` reports one in
/// an error, and a raw number there would be the only place the CLI shows one.
pub fn format_epoch(seconds: i64) -> String {
    DateTime::from_timestamp(seconds, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| seconds.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::types::{
        EditedInfo, MessageChannel, MessageFile, MessageMetadata, SlackMessage,
    };

    fn sample_message_with_blocks_and_metadata() -> SlackMessage {
        SlackMessage {
            ts: "1700000000.000100".into(),
            user: Some("U123".into()),
            bot_id: None,
            username: None,
            text: "hello".into(),
            channel: None,
            thread_ts: None,
            reply_count: Some(2),
            reply_users: None,
            reply_users_count: None,
            latest_reply: None,
            parent_user_id: None,
            reactions: None,
            subtype: None,
            edited: None,
            blocks: Some(vec![json!({"type": "section"})]),
            attachments: None,
            files: None,
            permalink: Some("https://acme.slack.com/archives/C123/p1".into()),
            metadata: Some(MessageMetadata {
                event_type: "deploy_done".into(),
                event_payload: json!({"version": "1.2.3"}),
            }),
        }
    }

    fn lean_fields() -> Vec<MessageField> {
        vec![
            MessageField::Ts,
            MessageField::User,
            MessageField::Text,
            MessageField::ReplyCount,
            MessageField::Metadata,
        ]
    }

    fn assert_names_agree<F>(name: fn(F) -> &'static str)
    where
        F: clap::ValueEnum + Serialize + Copy + std::fmt::Debug,
    {
        for &field in F::value_variants() {
            let flag = field.to_possible_value().expect("no field is hidden");
            assert_eq!(flag.get_name(), name(field), "{field:?} flag name");
            assert_eq!(
                serde_json::to_value(field).unwrap(),
                json!(name(field)),
                "{field:?} config name"
            );
        }
    }

    #[test]
    fn flags_config_and_output_keys_share_one_vocabulary() {
        assert_names_agree(UserField::as_str);
        assert_names_agree(ChannelField::as_str);
        assert_names_agree(MessageField::as_str);
    }

    #[test]
    fn message_fields_name_every_serialized_message_key() {
        let message = SlackMessage {
            ts: "1700000000.000100".into(),
            user: Some("U1".into()),
            bot_id: Some("B1".into()),
            username: Some("deploy-bot".into()),
            text: "hello".into(),
            channel: Some(MessageChannel {
                id: "C1".into(),
                name: None,
            }),
            thread_ts: Some("1700000000.000100".into()),
            reply_count: Some(1),
            reply_users: Some(vec!["U2".into()]),
            reply_users_count: Some(1),
            latest_reply: Some("1700000001.000100".into()),
            parent_user_id: Some("U1".into()),
            reactions: Some(vec![]),
            subtype: Some("bot_message".into()),
            edited: Some(EditedInfo {
                user: "U1".into(),
                ts: "1700000002.000100".into(),
            }),
            blocks: Some(vec![]),
            attachments: Some(vec![]),
            files: Some(vec![MessageFile {
                id: "F1".into(),
                name: None,
                title: None,
                mimetype: None,
                size: None,
                mode: Some("tombstone".into()),
                permalink: None,
            }]),
            permalink: Some("https://acme.slack.com/archives/C1/p1".into()),
            metadata: Some(MessageMetadata {
                event_type: "deploy_done".into(),
                event_payload: json!({}),
            }),
        };
        let serialized: HashSet<String> = serde_json::to_value(&message)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let selectable: HashSet<String> = <MessageField as clap::ValueEnum>::value_variants()
            .iter()
            .filter(|field| !matches!(field, MessageField::Date | MessageField::UserName))
            .map(|field| field.as_str().to_owned())
            .collect();

        assert_eq!(serialized, selectable);
    }

    #[test]
    fn project_message_drops_fields_outside_allowed_set() {
        let msg = sample_message_with_blocks_and_metadata();
        let allowed = field_names(&lean_fields());
        let projected = project_message(&msg, &allowed, None).unwrap();

        assert!(projected.get("blocks").is_none());
        assert!(projected.get("permalink").is_none());
        assert_eq!(projected["ts"], json!("1700000000.000100"));
        assert_eq!(projected["metadata"]["event_type"], json!("deploy_done"));
    }

    #[test]
    fn project_message_includes_blocks_when_allowed() {
        let msg = sample_message_with_blocks_and_metadata();
        let mut fields = lean_fields();
        fields.push(MessageField::Blocks);
        let allowed = field_names(&fields);
        let projected = project_message(&msg, &allowed, None).unwrap();

        assert_eq!(projected["blocks"][0]["type"], json!("section"));
    }

    #[test]
    fn project_message_adds_computed_date_when_requested() {
        let msg = sample_message_with_blocks_and_metadata();
        let mut fields = lean_fields();
        fields.push(MessageField::Date);
        let allowed = field_names(&fields);
        let projected = project_message(&msg, &allowed, None).unwrap();

        assert!(projected.get("date").is_some());
    }
}
