use crate::slack::types::{SlackChannel, SlackMessage, SlackUser};

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
