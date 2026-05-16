use std::io::{IsTerminal, Write};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use secrecy::ExposeSecret;

use crate::cli::AuthAction;
use crate::config::Config;
use crate::slack::SlackClient;

use super::Authenticator;
use super::login::{pkce_login, static_login};
use super::method::AuthMethod;
use super::oauth::callback::DEFAULT_CALLBACK_PORT;
use super::profile::{Profile, TokenSet};
use super::secret::{self, Secret, mask as mask_secret};

pub async fn handle(
    action: AuthAction,
    profile: Option<String>,
    config: Config,
    authenticator: Arc<Authenticator>,
    json: bool,
) -> Result<()> {
    match action {
        AuthAction::Login {
            method,
            user_token,
            bot_token,
            client_id,
            port,
            no_browser,
        } => {
            let input = LoginInput {
                method: method.map(Into::into),
                profile: profile.and_then(non_blank),
                user_token: user_token.and_then(non_blank).map(secret::new),
                bot_token: bot_token.and_then(non_blank).map(secret::new),
                client_id: client_id.and_then(non_blank),
                port: port.unwrap_or(DEFAULT_CALLBACK_PORT),
                no_browser,
            };
            let slack = SlackClient::new(config.clone(), authenticator.clone())?;
            login(input, config, &slack, &authenticator, json).await
        }

        AuthAction::Logout { all, keep_remote } => {
            let slack = if keep_remote {
                None
            } else {
                Some(SlackClient::new(config, authenticator.clone())?)
            };
            logout(profile, all, slack.as_ref(), &authenticator, json).await
        }

        AuthAction::Status { verify } => {
            let slack = if verify {
                Some(SlackClient::new(config, authenticator.clone())?)
            } else {
                None
            };
            status(profile, slack.as_ref(), &authenticator, json).await
        }

        AuthAction::Profiles => list_profiles(&authenticator, json).await,

        AuthAction::Use { name } => set_active(name, &authenticator, json).await,
    }
}

struct LoginInput {
    method: Option<AuthMethod>,
    profile: Option<String>,
    user_token: Option<Secret>,
    bot_token: Option<Secret>,
    client_id: Option<String>,
    port: u16,
    no_browser: bool,
}

async fn login(
    input: LoginInput,
    config: Config,
    slack: &SlackClient,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let method = decide_method(&input)?;

    let profile = match method {
        AuthMethod::Static => {
            let (user, bot) = collect_static_tokens(input.user_token, input.bot_token)?;
            static_login::run(user, bot, slack).await?
        }
        AuthMethod::Pkce => {
            let request = pkce_login::Request {
                client_id: input
                    .client_id
                    .context("PKCE login requires --client-id or SLACK_CLI_CLIENT_ID")?,
                api_base_url: config.connection.api_base_url.clone(),
                port: input.port,
                no_browser: input.no_browser,
            };
            pkce_login::run(request).await?
        }
    };

    let auto_named = input.profile.is_none();
    let profile_name = input
        .profile
        .unwrap_or_else(|| slugify(&profile.workspace.team_name));

    if auto_named {
        let snapshot = authenticator.snapshot().await;
        if let Some(existing) = snapshot.profiles.get(&profile_name)
            && existing.workspace.team_id != profile.workspace.team_id
        {
            anyhow::bail!(
                "profile '{profile_name}' already maps to team '{}' ({}); \
                 re-run with --profile NAME to save '{}' ({}) under a distinct name",
                existing.workspace.team_name,
                existing.workspace.team_id,
                profile.workspace.team_name,
                profile.workspace.team_id,
            );
        }
    }

    authenticator
        .upsert_profile(&profile_name, profile.clone(), true)
        .await?;

    print_login_result(&profile_name, &profile, json);
    Ok(())
}

fn decide_method(input: &LoginInput) -> Result<AuthMethod> {
    if let Some(method) = input.method {
        return Ok(method);
    }
    if input.user_token.is_some() || input.bot_token.is_some() {
        return Ok(AuthMethod::Static);
    }
    if std::io::stdin().is_terminal() {
        Ok(AuthMethod::Pkce)
    } else {
        Err(anyhow!(
            "no authentication method selected. pass --method pkce|static, \
             provide --user-token/--bot-token, or run interactively"
        ))
    }
}

fn collect_static_tokens(
    user: Option<Secret>,
    bot: Option<Secret>,
) -> Result<(Option<Secret>, Option<Secret>)> {
    let (user_token, bot_token) = if user.is_some() || bot.is_some() {
        (user, bot)
    } else if std::io::stdin().is_terminal() {
        let user =
            prompt("User token (xoxp-..., recommended; leave blank to skip): ")?.map(secret::new);
        let bot = prompt("Bot token (xoxb-..., optional; leave blank to skip): ")?.map(secret::new);
        (user, bot)
    } else {
        return Err(anyhow!(
            "static login requires --user-token or --bot-token in a non-interactive shell"
        ));
    };

    if user_token.is_none() && bot_token.is_none() {
        return Err(anyhow!(
            "at least one of --user-token or --bot-token is required"
        ));
    }

    Ok((user_token, bot_token))
}

fn prompt(label: &str) -> Result<Option<String>> {
    print!("{label}");
    std::io::stdout().flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    let trimmed = buf.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

fn non_blank(s: String) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn slugify(input: &str) -> String {
    let joined: String = input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if joined.is_empty() {
        "workspace".to_string()
    } else {
        joined
    }
}

fn print_login_result(profile_name: &str, profile: &Profile, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "profile": profile_name,
                "team_id": profile.workspace.team_id,
                "team_name": profile.workspace.team_name,
                "method": profile.method.as_str(),
            })
        );
    } else {
        println!(
            "✓ Logged in to {} via {} (profile: {})",
            profile.label(),
            profile.method,
            profile_name,
        );
    }
}

async fn logout(
    profile: Option<String>,
    all: bool,
    slack: Option<&SlackClient>,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let snapshot = authenticator.snapshot().await;

    let outcome = if all {
        if let Some(client) = slack {
            for profile in snapshot.profiles.values() {
                revoke_quietly(client, &profile.tokens).await;
            }
        }
        authenticator.clear_all().await?;
        LogoutOutcome::All
    } else {
        let target = profile
            .or_else(|| snapshot.active_profile.clone())
            .context("no active profile to log out from")?;

        if let Some(client) = slack
            && let Some(p) = snapshot.profiles.get(&target)
        {
            revoke_quietly(client, &p.tokens).await;
        }

        let found = authenticator.remove_profile(&target).await?.is_some();
        let was_active = snapshot.active_profile.as_deref() == Some(target.as_str());
        let new_active = authenticator.snapshot().await.active_profile;

        LogoutOutcome::Single {
            name: target,
            found,
            was_active,
            new_active,
        }
    };

    emit_logout_result(json, outcome);
    Ok(())
}

enum LogoutOutcome {
    All,
    Single {
        name: String,
        found: bool,
        was_active: bool,
        new_active: Option<String>,
    },
}

fn emit_logout_result(json: bool, outcome: LogoutOutcome) {
    if json {
        let payload = match &outcome {
            LogoutOutcome::All => serde_json::json!({"scope": "all"}),
            LogoutOutcome::Single {
                name,
                found,
                was_active,
                new_active,
            } => serde_json::json!({
                "scope": "single",
                "profile": name,
                "found": found,
                "was_active": was_active,
                "active_profile": new_active,
            }),
        };
        println!("{payload}");
        return;
    }

    match outcome {
        LogoutOutcome::All => println!("✓ Removed all profiles"),
        LogoutOutcome::Single {
            name,
            found,
            was_active,
            new_active,
        } => {
            if found {
                println!("✓ Removed profile {name}");
            } else {
                println!("Profile {name} was not found");
            }
            if was_active && new_active.is_none() {
                println!("  No active profile. Run: slack-cli auth use <NAME>");
            }
        }
    }
}

async fn revoke_quietly(slack: &SlackClient, tokens: &TokenSet) {
    let token = tokens
        .user
        .as_ref()
        .or(tokens.bot.as_ref())
        .map(|s| s.expose_secret().to_string());
    if let Some(token) = token
        && let Err(err) = slack.auth.revoke(&token).await
    {
        tracing::warn!("auth.revoke failed: {err}");
    }
}

async fn status(
    profile: Option<String>,
    slack: Option<&SlackClient>,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let snapshot = authenticator.snapshot().await;
    if snapshot.profiles.is_empty() {
        if json {
            println!("{}", serde_json::json!({"profiles": []}));
        } else {
            println!("No profiles configured. Run: slack-cli auth login");
        }
        return Ok(());
    }

    let name = profile
        .or_else(|| snapshot.active_profile.clone())
        .context("no active profile selected")?;
    let profile = snapshot
        .profiles
        .get(&name)
        .with_context(|| format!("profile {name} not found"))?;

    let verification = match slack {
        Some(client) => {
            let token = profile
                .tokens
                .user
                .as_ref()
                .or(profile.tokens.bot.as_ref())
                .map(|s| s.expose_secret().to_string());
            match token {
                Some(t) => Some(client.auth.test(&t).await),
                None => None,
            }
        }
        None => None,
    };

    if json {
        print_status_json(&name, &snapshot.active_profile, profile, verification);
    } else {
        print_status_text(&name, profile, verification);
    }

    Ok(())
}

fn print_status_json(
    name: &str,
    active: &Option<String>,
    profile: &Profile,
    verification: Option<anyhow::Result<crate::slack::SlackAuthIdentity>>,
) {
    let mut payload = serde_json::json!({
        "profile": name,
        "active": active.as_deref() == Some(name),
        "method": profile.method.as_str(),
        "workspace": profile.workspace,
        "tokens": {
            "user": profile.tokens.user.as_ref().map(mask_secret),
            "bot":  profile.tokens.bot.as_ref().map(mask_secret),
        },
        "scopes": profile.scopes,
        "authorized_at": profile.authorized_at,
    });
    match verification {
        Some(Ok(identity)) => {
            payload["verified"] = serde_json::json!({
                "team": identity.team,
                "team_id": identity.team_id,
                "user": identity.user,
                "user_id": identity.user_id,
            });
        }
        Some(Err(err)) => {
            payload["verified"] = serde_json::json!({"error": err.to_string()});
        }
        None => {}
    }
    println!("{payload}");
}

fn print_status_text(
    name: &str,
    profile: &Profile,
    verification: Option<anyhow::Result<crate::slack::SlackAuthIdentity>>,
) {
    println!("profile: {name} ({})", profile.method);
    println!(
        "  workspace: {} ({})",
        profile.workspace.team_name, profile.workspace.team_id
    );
    if let Some(token) = &profile.tokens.user {
        println!("  user_token: {}", mask_secret(token));
    }
    if let Some(token) = &profile.tokens.bot {
        println!("  bot_token : {}", mask_secret(token));
    }
    if !profile.scopes.is_empty() {
        println!("  scopes    : {}", profile.scopes.join(", "));
    }
    if let Some(result) = verification {
        match result {
            Ok(identity) => println!("  verified  : ok ({} / {})", identity.team, identity.user),
            Err(err) => println!("  verified  : failed ({err})"),
        }
    }
}

async fn list_profiles(authenticator: &Authenticator, json: bool) -> Result<()> {
    let snapshot = authenticator.snapshot().await;
    if json {
        let payload: Vec<_> = snapshot
            .profiles
            .iter()
            .map(|(name, profile)| {
                serde_json::json!({
                    "name": name,
                    "active": snapshot.active_profile.as_deref() == Some(name),
                    "method": profile.method.as_str(),
                    "workspace": profile.workspace,
                })
            })
            .collect();
        println!("{}", serde_json::json!({"profiles": payload}));
    } else if snapshot.profiles.is_empty() {
        println!("No profiles configured. Run: slack-cli auth login");
    } else {
        for (name, profile) in &snapshot.profiles {
            let marker = if snapshot.active_profile.as_deref() == Some(name.as_str()) {
                "*"
            } else {
                " "
            };
            println!(
                "{} {:<20} {:<8} {}",
                marker,
                name,
                profile.method,
                profile.label()
            );
        }
    }
    Ok(())
}

async fn set_active(name: String, authenticator: &Authenticator, json: bool) -> Result<()> {
    let name = non_blank(name).context("profile name must not be blank")?;
    authenticator.set_active(&name).await?;
    if json {
        println!("{}", serde_json::json!({"active": name}));
    } else {
        println!("✓ Active profile: {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_dashes_non_alnum() {
        assert_eq!(slugify("Acme Inc."), "acme-inc");
        assert_eq!(slugify("My Team!"), "my-team");
    }

    #[test]
    fn slugify_collapses_repeated_separators() {
        assert_eq!(slugify("foo---bar"), "foo-bar");
        assert_eq!(slugify("  foo  bar  "), "foo-bar");
    }

    #[test]
    fn slugify_falls_back_for_all_non_ascii_input() {
        assert_eq!(slugify("한국팀"), "workspace");
        assert_eq!(slugify(""), "workspace");
    }

    #[test]
    fn non_blank_trims_and_rejects_empty() {
        assert_eq!(non_blank("  ".into()), None);
        assert_eq!(non_blank("".into()), None);
        assert_eq!(non_blank("  abc  ".into()), Some("abc".to_string()));
    }
}
