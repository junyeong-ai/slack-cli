use crate::slack::types::{SlackChannel, SlackMessage, SlackUser};
use crate::slack::{Bookmark, CustomEmoji, MessageReactions, PinnedMessage};

pub fn print_users(users: &[SlackUser], as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(users) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing users: {}", e),
        }
        return;
    }

    if users.is_empty() {
        println!("No users found");
        return;
    }

    for user in users {
        let status = if user.deleted {
            "deleted"
        } else if user.is_bot {
            "bot"
        } else {
            "active"
        };

        let email = user
            .profile
            .as_ref()
            .and_then(|p| p.email.as_deref())
            .unwrap_or("-");

        println!(
            "{:<20} {:<30} {:<30} {}",
            user.name,
            user.real_name().unwrap_or("-"),
            email,
            status
        );
    }
}

pub fn print_channels(channels: &[SlackChannel], as_json: bool) {
    if as_json {
        match serde_json::to_string_pretty(channels) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Error serializing channels: {}", e),
        }
        return;
    }

    if channels.is_empty() {
        println!("No channels found");
        return;
    }

    for ch in channels {
        let typ = if ch.is_im {
            "DM"
        } else if ch.is_mpim {
            "Group"
        } else if ch.is_private {
            "Private"
        } else {
            "Public"
        };

        let topic = ch.topic.as_ref().map(|t| t.value.as_str()).unwrap_or("");

        println!(
            "{:<20} {:<8} {:>4} members  {}",
            ch.name,
            typ,
            ch.num_members.unwrap_or(0),
            topic
        );
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
        println!(
            "[{}] {}: {}",
            msg.ts,
            msg.user.as_deref().unwrap_or("system"),
            msg.text
        );

        if let Some(count) = msg.reply_count {
            println!("  └─ {} replies", count);
        }
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
