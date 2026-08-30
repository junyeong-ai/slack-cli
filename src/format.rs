use crate::cache::SqliteCache;
use crate::events::{DaemonStatus, Event, PruneOutcome, StoreCaps, StoreStats};
use crate::slack::types::{SlackChannel, SlackMessage, SlackUser};
use crate::slack::{
    Bookmark, CustomEmoji, MessageReactions, PinnedMessage, SearchCapabilities, SearchResults,
};
use crate::update::install::Signature;
use crate::update::{Action, UpdateOutcome};
use chrono::DateTime;
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn print_users(users: &[SlackUser], fields: &[String], as_json: bool) {
    if users.is_empty() {
        if as_json {
            println!("[]");
        } else {
            println!("No users found");
        }
        return;
    }

    if as_json {
        let filtered: Vec<Value> = users
            .iter()
            .map(|u| filter_user_fields(u, fields))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&filtered).unwrap_or_default()
        );
        return;
    }

    for user in users {
        let mut parts: Vec<String> = Vec::new();

        for field in fields {
            let value = get_user_field(user, field);
            parts.push(value);
        }

        println!("{}", parts.join("\t"));
    }
}

fn filter_user_fields(user: &SlackUser, fields: &[String]) -> Value {
    let mut obj = serde_json::Map::new();

    for field in fields {
        match field.as_str() {
            "id" => {
                obj.insert("id".to_string(), json!(user.id));
            }
            "name" => {
                obj.insert("name".to_string(), json!(user.name));
            }
            "real_name" => {
                let v = user.profile.as_ref().and_then(|p| p.real_name.as_ref());
                obj.insert("real_name".to_string(), json!(v));
            }
            "display_name" => {
                let v = user.profile.as_ref().and_then(|p| p.display_name.as_ref());
                obj.insert("display_name".to_string(), json!(v));
            }
            "email" => {
                let v = user.profile.as_ref().and_then(|p| p.email.as_ref());
                obj.insert("email".to_string(), json!(v));
            }
            "status" => {
                let text = user.profile.as_ref().and_then(|p| p.status_text.as_ref());
                let emoji = user.profile.as_ref().and_then(|p| p.status_emoji.as_ref());
                let status = match (text, emoji) {
                    (Some(t), Some(e)) if !t.is_empty() => format!("{} {}", e, t),
                    (Some(t), _) if !t.is_empty() => t.clone(),
                    (_, Some(e)) if !e.is_empty() => e.clone(),
                    _ => String::new(),
                };
                obj.insert("status".to_string(), json!(status));
            }
            "status_emoji" => {
                let v = user.profile.as_ref().and_then(|p| p.status_emoji.as_ref());
                obj.insert("status_emoji".to_string(), json!(v));
            }
            "avatar" => {
                let v = user.profile.as_ref().and_then(|p| p.avatar.as_ref());
                obj.insert("avatar".to_string(), json!(v));
            }
            "title" => {
                let v = user.profile.as_ref().and_then(|p| p.title.as_ref());
                obj.insert("title".to_string(), json!(v));
            }
            "timezone" => {
                let v = user.profile.as_ref().and_then(|p| p.timezone.as_ref());
                obj.insert("timezone".to_string(), json!(v));
            }
            "is_admin" => {
                obj.insert("is_admin".to_string(), json!(user.is_admin));
            }
            "is_bot" => {
                obj.insert("is_bot".to_string(), json!(user.is_bot));
            }
            "deleted" => {
                obj.insert("deleted".to_string(), json!(user.deleted));
            }
            _ => {}
        }
    }

    Value::Object(obj)
}

fn get_user_field(user: &SlackUser, field: &str) -> String {
    match field {
        "id" => user.id.clone(),
        "name" => user.name.clone(),
        "real_name" => user
            .profile
            .as_ref()
            .and_then(|p| p.real_name.clone())
            .unwrap_or_else(|| "-".to_string()),
        "display_name" => user
            .profile
            .as_ref()
            .and_then(|p| p.display_name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "-".to_string()),
        "email" => user
            .profile
            .as_ref()
            .and_then(|p| p.email.clone())
            .unwrap_or_else(|| "-".to_string()),
        "status" => {
            let text = user.profile.as_ref().and_then(|p| p.status_text.as_ref());
            let emoji = user.profile.as_ref().and_then(|p| p.status_emoji.as_ref());
            match (text, emoji) {
                (Some(t), Some(e)) if !t.is_empty() => format!("{} {}", e, t),
                (Some(t), _) if !t.is_empty() => t.clone(),
                (_, Some(e)) if !e.is_empty() => e.clone(),
                _ => "-".to_string(),
            }
        }
        "status_emoji" => user
            .profile
            .as_ref()
            .and_then(|p| p.status_emoji.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "-".to_string()),
        "avatar" => user
            .profile
            .as_ref()
            .and_then(|p| p.avatar.clone())
            .unwrap_or_else(|| "-".to_string()),
        "title" => user
            .profile
            .as_ref()
            .and_then(|p| p.title.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "-".to_string()),
        "timezone" => user
            .profile
            .as_ref()
            .and_then(|p| p.timezone.clone())
            .unwrap_or_else(|| "-".to_string()),
        "is_admin" => if user.is_admin { "admin" } else { "-" }.to_string(),
        "is_bot" => if user.is_bot { "bot" } else { "-" }.to_string(),
        "deleted" => if user.deleted { "deleted" } else { "-" }.to_string(),
        _ => "-".to_string(),
    }
}

pub fn print_channels(channels: &[SlackChannel], fields: &[String], as_json: bool) {
    if channels.is_empty() {
        if as_json {
            println!("[]");
        } else {
            println!("No channels found");
        }
        return;
    }

    if as_json {
        let filtered: Vec<Value> = channels
            .iter()
            .map(|c| filter_channel_fields(c, fields))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&filtered).unwrap_or_default()
        );
        return;
    }

    for ch in channels {
        let mut parts: Vec<String> = Vec::new();

        for field in fields {
            let value = get_channel_field(ch, field);
            parts.push(value);
        }

        println!("{}", parts.join("\t"));
    }
}

fn filter_channel_fields(ch: &SlackChannel, fields: &[String]) -> Value {
    let mut obj = serde_json::Map::new();

    for field in fields {
        match field.as_str() {
            "id" => {
                obj.insert("id".to_string(), json!(ch.id));
            }
            "name" => {
                obj.insert("name".to_string(), json!(ch.name));
            }
            "user" => {
                obj.insert("user".to_string(), json!(ch.user));
            }
            "type" => {
                let typ = get_channel_type(ch);
                obj.insert("type".to_string(), json!(typ));
            }
            "members" => {
                obj.insert("members".to_string(), json!(ch.num_members));
            }
            "topic" => {
                let v = ch.topic.as_ref().map(|t| &t.value);
                obj.insert("topic".to_string(), json!(v));
            }
            "purpose" => {
                let v = ch.purpose.as_ref().map(|p| &p.value);
                obj.insert("purpose".to_string(), json!(v));
            }
            "created" => {
                obj.insert("created".to_string(), json!(ch.created));
            }
            "creator" => {
                obj.insert("creator".to_string(), json!(ch.creator));
            }
            "is_member" => {
                obj.insert("is_member".to_string(), json!(ch.is_member));
            }
            "is_archived" => {
                obj.insert("is_archived".to_string(), json!(ch.is_archived));
            }
            "is_private" => {
                obj.insert("is_private".to_string(), json!(ch.is_private));
            }
            _ => {}
        }
    }

    Value::Object(obj)
}

fn get_channel_type(ch: &SlackChannel) -> &'static str {
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

fn get_channel_field(ch: &SlackChannel, field: &str) -> String {
    match field {
        "id" => ch.id.clone(),
        "name" => ch.name.clone().unwrap_or_else(|| "-".to_string()),
        "user" => ch.user.clone().unwrap_or_else(|| "-".to_string()),
        "type" => get_channel_type(ch).to_string(),
        "members" => ch
            .num_members
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string()),
        "topic" => ch
            .topic
            .as_ref()
            .map(|t| t.value.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "-".to_string()),
        "purpose" => ch
            .purpose
            .as_ref()
            .map(|p| p.value.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "-".to_string()),
        "created" => ch
            .created
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| ts.to_string())
            })
            .unwrap_or_else(|| "-".to_string()),
        "creator" => ch.creator.clone().unwrap_or_else(|| "-".to_string()),
        "is_member" => if ch.is_member { "member" } else { "-" }.to_string(),
        "is_archived" => if ch.is_archived { "archived" } else { "-" }.to_string(),
        "is_private" => if ch.is_private { "private" } else { "public" }.to_string(),
        _ => "-".to_string(),
    }
}

pub fn print_messages(
    messages: &[SlackMessage],
    as_json: bool,
    fields: &[String],
    cache: Option<&SqliteCache>,
) {
    let allowed: HashSet<&str> = fields.iter().map(String::as_str).collect();

    if as_json {
        let projected = project_messages(messages, &allowed, cache);
        match serde_json::to_string_pretty(&projected) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing messages: {}", e),
        }
        return;
    }

    render_messages(messages, &allowed, cache);
}

/// One page of `conversations.history` plus the cursor to the next page.
/// The JSON shape is an envelope — `{messages, next_cursor}` with a null
/// cursor on the last page — because following the cursor is the caller's
/// job for this command. Internally-paginating commands (`thread`) keep the
/// bare array shape of `print_messages`.
pub fn print_history(
    messages: &[SlackMessage],
    next_cursor: Option<&str>,
    as_json: bool,
    fields: &[String],
    cache: Option<&SqliteCache>,
) {
    let allowed: HashSet<&str> = fields.iter().map(String::as_str).collect();

    if as_json {
        let envelope = json!({
            "messages": project_messages(messages, &allowed, cache),
            "next_cursor": next_cursor,
        });
        match serde_json::to_string_pretty(&envelope) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing messages: {}", e),
        }
        return;
    }

    render_messages(messages, &allowed, cache);
    if let Some(cursor) = next_cursor {
        eprintln!("More messages available: rerun with --cursor {}", cursor);
    }
}

fn project_messages(
    messages: &[SlackMessage],
    allowed: &HashSet<&str>,
    cache: Option<&SqliteCache>,
) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| project_message(msg, allowed, cache))
        .collect()
}

fn render_messages(
    messages: &[SlackMessage],
    allowed: &HashSet<&str>,
    cache: Option<&SqliteCache>,
) {
    if messages.is_empty() {
        println!("No messages found");
        return;
    }

    let expand_date = allowed.contains("date");
    let expand_user_name = allowed.contains("user_name");

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

        println!("[{}] {}: {}", ts_display, author, msg.text);

        // Render attachments (wee-slack style)
        if let Some(attachments) = &msg.attachments {
            for att in attachments {
                render_attachment(att);
            }
        }

        if let Some(count) = msg.reply_count {
            println!("  └─ {} replies", count);
        }
    }
}

fn project_message(
    msg: &SlackMessage,
    allowed: &HashSet<&str>,
    cache: Option<&SqliteCache>,
) -> Value {
    let mut value = serde_json::to_value(msg).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut value {
        map.retain(|key, _| allowed.contains(key.as_str()));

        if allowed.contains("date")
            && let Some(date_str) = format_timestamp(&msg.ts)
        {
            map.insert("date".to_string(), json!(date_str));
        }
        if allowed.contains("user_name")
            && let Some(user_id) = &msg.user
            && let Some(name) = resolve_user_name(user_id, cache)
        {
            map.insert("user_name".to_string(), json!(name));
        }
    }
    value
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

fn render_attachment(att: &Value) {
    let mut rendered = false;

    if let Some(pretext) = att.get("pretext").and_then(|v| v.as_str())
        && !pretext.is_empty()
    {
        println!("  │ {}", pretext);
        rendered = true;
    }

    let author = att.get("author_name").and_then(|v| v.as_str());
    let title = att.get("title").and_then(|v| v.as_str());
    match (author, title) {
        (Some(a), Some(t)) => {
            println!("  │ {}: {}", a, t);
            rendered = true;
        }
        (Some(a), None) => {
            println!("  │ {}", a);
            rendered = true;
        }
        (None, Some(t)) => {
            println!("  │ {}", t);
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
                println!("  │ {}", trimmed);
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
                    println!("  │ {}: {}", t, first_line);
                    rendered = true;
                }
                (None, Some(v)) => {
                    let first_line = v.lines().next().unwrap_or(v);
                    println!("  │ {}", first_line);
                    rendered = true;
                }
                _ => {}
            }
        }
    }

    if let Some(footer) = att.get("footer").and_then(|v| v.as_str())
        && !footer.is_empty()
    {
        println!("  │ {}", footer);
        rendered = true;
    }

    if !rendered
        && let Some(fallback) = att.get("fallback").and_then(|v| v.as_str())
        && !fallback.is_empty()
    {
        println!("  │ {}", fallback);
    }
}

pub fn print_members(member_ids: &[String], cache: &crate::cache::SqliteCache, as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(member_ids) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing members: {}", e),
        }
        return;
    }

    if member_ids.is_empty() {
        println!("No members found");
        return;
    }

    let users = cache.get_users().unwrap_or_default();

    for id in member_ids {
        if let Some(user) = users.iter().find(|u| &u.id == id) {
            println!("{:<20} {}", user.name, id);
        } else {
            println!("{}", id);
        }
    }
}

pub fn print_reactions(reactions: &MessageReactions, as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(reactions) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing reactions: {}", e),
        }
        return;
    }

    if reactions.reactions.is_empty() {
        println!("No reactions");
        return;
    }

    for r in &reactions.reactions {
        println!(":{}: ({})", r.name, r.count);
    }
}

pub fn print_emoji(emoji: &[CustomEmoji], as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(emoji) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing emoji: {}", e),
        }
        return;
    }

    if emoji.is_empty() {
        println!("No custom emoji found");
        return;
    }

    for e in emoji {
        if e.is_alias {
            println!(
                ":{}: -> :{}: (alias)",
                e.name,
                e.alias_for.as_deref().unwrap_or("?")
            );
        } else {
            println!(":{}: {}", e.name, e.url);
        }
    }
}

pub fn print_pins(pins: &[PinnedMessage], as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(pins) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing pins: {}", e),
        }
        return;
    }

    if pins.is_empty() {
        println!("No pinned messages");
        return;
    }

    for pin in pins {
        let text = pin.text.as_deref().unwrap_or("[no text]");
        let preview: String = text.chars().take(60).collect();
        if text.chars().count() > 60 {
            println!("[{}] {}...", pin.ts, preview);
        } else {
            println!("[{}] {}", pin.ts, preview);
        }
    }
}

pub fn print_bookmarks(bookmarks: &[Bookmark], as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(bookmarks) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing bookmarks: {}", e),
        }
        return;
    }

    if bookmarks.is_empty() {
        println!("No bookmarks");
        return;
    }

    for b in bookmarks {
        let emoji = b.emoji.as_deref().unwrap_or("");
        println!("{} {} - {} (id: {})", emoji, b.title, b.link, b.id);
    }
}

pub fn print_search_capabilities(capabilities: &SearchCapabilities, as_json: bool) {
    if as_json {
        println!(
            "{}",
            json!({ "is_ai_search_enabled": capabilities.is_ai_search_enabled })
        );
        return;
    }

    println!(
        "semantic search: {}",
        if capabilities.is_ai_search_enabled {
            "enabled"
        } else {
            "unavailable (keyword matching only)"
        }
    );
}

pub fn print_search_results(results: &SearchResults, as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(results) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing search results: {}", e),
        }
        return;
    }

    if results.messages.is_empty()
        && results.files.is_empty()
        && results.channels.is_empty()
        && results.users.is_empty()
    {
        println!("No search results found");
        return;
    }

    for msg in &results.messages {
        let author = msg
            .author_name
            .as_deref()
            .or(msg.author_user_id.as_deref())
            .unwrap_or("unknown");
        let channel = msg.channel_name.as_deref().unwrap_or("-");
        println!("[message] #{} {}: {}", channel, author, msg.text);
        if let Some(permalink) = &msg.permalink {
            println!("  {}", permalink);
        }
    }

    for file in &results.files {
        let title = file.title.as_deref().unwrap_or("[untitled file]");
        let file_type = file.file_type.as_deref().unwrap_or("file");
        println!("[file] {} ({})", title, file_type);
        if let Some(permalink) = &file.permalink {
            println!("  {}", permalink);
        }
    }

    for channel in &results.channels {
        let name = channel.name.as_deref().unwrap_or("[unnamed channel]");
        println!("[channel] #{}", name);
        if let Some(topic) = channel.topic.as_deref().filter(|topic| !topic.is_empty()) {
            println!("  {}", topic);
        }
        if let Some(permalink) = &channel.permalink {
            println!("  {}", permalink);
        }
    }

    for user in &results.users {
        let name = user
            .full_name
            .as_deref()
            .or(user.user_id.as_deref())
            .unwrap_or("[unknown user]");
        println!("[user] {}", name);
        if let Some(title) = user.title.as_deref().filter(|title| !title.is_empty()) {
            println!("  {}", title);
        }
        if let Some(permalink) = &user.permalink {
            println!("  {}", permalink);
        }
    }
}

pub fn print_update_outcome(outcome: &UpdateOutcome, as_json: bool) {
    let signature = outcome.signature.map(|signature| match signature {
        Signature::Verified => "verified",
        Signature::Unverified => "unverified",
    });

    if as_json {
        println!(
            "{}",
            json!({
                "action": outcome.action.as_str(),
                "from": outcome.from,
                "to": outcome.to,
                "target": outcome.target,
                "binary": outcome.binary.display().to_string(),
                "signature": signature,
            })
        );
        return;
    }

    match outcome.action {
        Action::AlreadyCurrent => println!("Already at v{} ({})", outcome.to, outcome.target),
        Action::UpdateAvailable => println!(
            "v{} is available (running v{}). Run: slack-cli self update",
            outcome.to, outcome.from
        ),
        Action::Cancelled => println!("Cancelled; still at v{}", outcome.from),
        Action::Updated | Action::Reinstalled => {
            println!(
                "✓ {} to v{} at {}",
                if outcome.action == Action::Reinstalled {
                    "Reinstalled"
                } else {
                    "Updated"
                },
                outcome.to,
                outcome.binary.display()
            );
            if signature == Some("unverified") {
                println!(
                    "  cosign is not installed, so the signature was not checked and the \
                     download rests on its checksum alone."
                );
            }
        }
    }
}

/// One JSON object per line, which is what a consuming process reads. Not
/// `to_string_pretty`: a pulled batch is a stream of records, and a record
/// that spans lines cannot be read one at a time.
pub fn print_events(events: &[Event], as_json: bool) {
    if as_json {
        for event in events {
            match event.to_ndjson() {
                Ok(line) => println!("{line}"),
                Err(err) => eprintln!("could not encode event {}: {err}", event.id),
            }
        }
        return;
    }

    if events.is_empty() {
        println!("No events");
        return;
    }

    for event in events {
        println!(
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
        );
    }
}

pub fn print_event_stats(
    stats: &StoreStats,
    caps: &StoreCaps,
    mode: &str,
    profile: &str,
    as_json: bool,
) {
    if as_json {
        println!(
            "{}",
            json!({
                "profile": profile,
                "mode": mode,
                "durable": caps.durable,
                "replayable": caps.replayable,
                "events": stats.events,
                "bytes": stats.bytes,
                "oldest": stats.oldest,
                "newest": stats.newest,
                "consumers": stats.consumers,
            })
        );
        return;
    }

    println!("profile : {profile}");
    println!("mode    : {mode}");
    if !caps.durable {
        println!("events  : not stored (streaming only)");
        return;
    }

    println!("events  : {}", stats.events);
    println!("size    : {}", human_bytes(stats.bytes));
    if let (Some(oldest), Some(newest)) = (stats.oldest, stats.newest) {
        println!("oldest  : {}", format_epoch(oldest));
        println!("newest  : {}", format_epoch(newest));
    }
    if stats.consumers.is_empty() {
        println!("consumers: none registered");
    } else {
        println!("consumers:");
        for consumer in &stats.consumers {
            println!(
                "  {:<20} acked {:<10} pending {}",
                consumer.name, consumer.acked_seq, consumer.pending
            );
        }
    }
}

pub fn print_daemon_status(
    status: Option<&DaemonStatus>,
    profile: &str,
    stale_after: i64,
    as_json: bool,
) {
    // Named on every line of output, because the events of one installation
    // live apart from another's: a command run without the environment the
    // daemon runs under reads a different store, finds it empty, and would
    // otherwise report a working daemon as absent.
    let Some(status) = status else {
        if as_json {
            println!("{}", json!({ "running": false, "profile": profile }));
        } else {
            println!("profile : {profile}");
            println!(
                "running : no daemon has run for this profile. Start one: slack-cli daemon run"
            );
        }
        return;
    };

    // A heartbeat that stopped is how a killed daemon shows up: the record it
    // left behind is still there, and only its age says it is gone.
    let age = chrono::Utc::now().timestamp() - status.heartbeat_at;
    let live = age <= stale_after;

    if as_json {
        println!(
            "{}",
            json!({
                "running": live,
                "profile": profile,
                "pid": status.pid,
                "connected": status.connected && live,
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
            })
        );
        return;
    }

    println!("profile  : {profile}");
    if live {
        println!(
            "running  : pid {} ({}), {}",
            status.pid,
            if status.connected {
                "connected"
            } else {
                "reconnecting"
            },
            format_epoch(status.started_at)
        );
    } else {
        println!(
            "stopped  : last heartbeat {}s ago (pid {} was running since {})",
            age,
            status.pid,
            format_epoch(status.started_at)
        );
    }

    let counters = &status.counters;
    println!(
        "events   : {} received, {} matched, {} stored, {} recovered",
        counters.received, counters.matched, counters.stored, counters.backfilled
    );
    println!(
        "delivery : {} delivered, {} failed, {} reconnects",
        counters.delivered, counters.failed, counters.reconnects
    );
    if counters.dropped > 0 {
        println!(
            "dropped  : {} — the buffer overflowed. Raise events.buffer, or make the sink \
             faster: delivery is serial, so one slow handler holds the whole pipeline",
            counters.dropped
        );
    }
}

pub fn print_prune_outcome(outcome: &PruneOutcome, as_json: bool) {
    if as_json {
        println!(
            "{}",
            json!({
                "removed": outcome.total(),
                "acknowledged": outcome.acknowledged,
                "expired": outcome.expired,
                "over_budget": outcome.over_budget,
            })
        );
        return;
    }

    println!(
        "✓ Removed {} events ({} acknowledged, {} expired, {} over the size budget)",
        outcome.total(),
        outcome.acknowledged,
        outcome.expired,
        outcome.over_budget
    );
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
    use crate::slack::types::{MessageMetadata, SlackMessage};

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
            permalink: Some("https://acme.slack.com/archives/C123/p1".into()),
            metadata: Some(MessageMetadata {
                event_type: "deploy_done".into(),
                event_payload: json!({"version": "1.2.3"}),
            }),
        }
    }

    fn lean_fields() -> Vec<String> {
        ["ts", "user", "text", "reply_count", "metadata"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn project_message_drops_fields_outside_allowed_set() {
        let msg = sample_message_with_blocks_and_metadata();
        let fields = lean_fields();
        let allowed: HashSet<&str> = fields.iter().map(String::as_str).collect();
        let projected = project_message(&msg, &allowed, None);

        assert!(projected.get("blocks").is_none());
        assert!(projected.get("permalink").is_none());
        assert_eq!(projected["ts"], json!("1700000000.000100"));
        assert_eq!(projected["metadata"]["event_type"], json!("deploy_done"));
    }

    #[test]
    fn project_message_includes_blocks_when_allowed() {
        let msg = sample_message_with_blocks_and_metadata();
        let mut fields = lean_fields();
        fields.push("blocks".into());
        let allowed: HashSet<&str> = fields.iter().map(String::as_str).collect();
        let projected = project_message(&msg, &allowed, None);

        assert_eq!(projected["blocks"][0]["type"], json!("section"));
    }

    #[test]
    fn project_message_adds_computed_date_when_requested() {
        let msg = sample_message_with_blocks_and_metadata();
        let mut fields = lean_fields();
        fields.push("date".into());
        let allowed: HashSet<&str> = fields.iter().map(String::as_str).collect();
        let projected = project_message(&msg, &allowed, None);

        assert!(projected.get("date").is_some());
    }
}
