use crate::slack::types::{SlackChannel, SlackMessage, SlackUser};
use crate::slack::{Bookmark, CustomEmoji, MessageReactions, PinnedMessage};
use serde_json::{Value, json};

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
        "name" => ch.name.clone(),
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

pub fn print_messages(messages: &[SlackMessage], as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(messages) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing messages: {}", e),
        }
        return;
    }

    if messages.is_empty() {
        println!("No messages found");
        return;
    }

    for msg in messages {
        // Priority: user > username (bot display name) > bot_id > "system"
        let author = msg
            .user
            .as_deref()
            .or(msg.username.as_deref())
            .or(msg.bot_id.as_deref())
            .unwrap_or("system");
        println!("[{}] {}: {}", msg.ts, author, msg.text);

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
