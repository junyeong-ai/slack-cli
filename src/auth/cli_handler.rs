use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use secrecy::ExposeSecret;
use serde_json::{Value, json};

use crate::cli::AuthAction;
use crate::config::{AuthConfig, Config};
use crate::slack::{SlackClient, scopes};

use super::Authenticator;
use super::credential::{Credential, TokenKind, TokenSet};
use super::login::{browser_login, static_login};
use super::method::AuthMethod;
use super::oauth::callback::DEFAULT_CALLBACK_PORT;
use super::oauth::client::OAuthClient;
use super::profile::Profile;
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

        AuthAction::Scopes => {
            print_scopes(&config.auth.exclude_scopes, json);
            Ok(())
        }
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

impl LoginInput {
    /// The command line and the environment win; `config.toml` supplies the
    /// client id when they left it out.
    fn with_stored_app(mut self, auth: &AuthConfig) -> Self {
        self.client_id = self.client_id.or_else(|| auth.client_id.clone());
        self
    }
}

async fn login(
    input: LoginInput,
    config: Config,
    slack: &SlackClient,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let method = decide_method(&input)?;
    let input = input.with_stored_app(&config.auth);

    let profile = match method {
        AuthMethod::Static => {
            let (user, bot) = collect_static_tokens(input.user_token, input.bot_token)?;
            static_login::run(user, bot, slack).await?
        }
        AuthMethod::Pkce => {
            let request = browser_login::Request {
                client: build_client(input.client_id)?,
                api_base_url: config.connection.api_base_url.clone(),
                port: input.port,
                no_browser: input.no_browser,
                user_scopes: owned(scopes::requested(
                    TokenKind::User,
                    &config.auth.exclude_scopes,
                )),
            };
            browser_login::run(request).await?
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

fn build_client(client_id: Option<String>) -> Result<OAuthClient> {
    let client_id = client_id.context(
        "browser login requires a client id: pass --client-id, set SLACK_CLI_CLIENT_ID, \
         or put client_id under [auth] in config.toml",
    )?;
    Ok(OAuthClient::new(client_id))
}

fn collect_static_tokens(
    user: Option<Secret>,
    bot: Option<Secret>,
) -> Result<(Option<Secret>, Option<Secret>)> {
    let (user_token, bot_token) = if user.is_some() || bot.is_some() {
        (user, bot)
    } else if std::io::stdin().is_terminal() {
        let user = prompt_secret("User token (xoxp-..., recommended; leave blank to skip): ")?
            .map(secret::new);
        let bot = prompt_secret("Bot token (xoxb-..., optional; leave blank to skip): ")?
            .map(secret::new);
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

/// Reads a token from the terminal without echoing it, so it never lands in
/// scrollback or a screen share.
fn prompt_secret(label: &str) -> Result<Option<String>> {
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let entered = rpassword::prompt_password(label)?;
    let trimmed = entered.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

fn non_blank(s: String) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn owned(scopes: Vec<&'static str>) -> Vec<String> {
    scopes.into_iter().map(ToOwned::to_owned).collect()
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

fn print_scopes(excluded: &[String], json: bool) {
    let user = scopes::requested(TokenKind::User, excluded);
    let bot = scopes::requested(TokenKind::Bot, excluded);
    if json {
        println!("{}", json!({ "user": user, "bot": bot }));
    } else {
        println!("User Token Scopes:");
        println!("  {}", user.join(" "));
        println!("Bot Token Scopes:");
        println!("  {}", bot.join(" "));
    }
}

fn print_login_result(profile_name: &str, profile: &Profile, json: bool) {
    if json {
        println!(
            "{}",
            json!({
                "profile": profile_name,
                "team_id": profile.workspace.team_id,
                "team_name": profile.workspace.team_name,
                "method": profile.method.as_str(),
                "tokens": token_kinds(&profile.tokens),
            })
        );
    } else {
        println!(
            "✓ Logged in to {} via {} (profile: {})",
            profile.label(),
            profile.method,
            profile_name,
        );
        for (kind, credential) in profile.tokens.iter() {
            println!("  {kind} token: {}", describe(credential));
        }
    }
}

fn token_kinds(tokens: &TokenSet) -> Vec<&'static str> {
    tokens.iter().map(|(kind, _)| kind.as_str()).collect()
}

fn describe(credential: &Credential) -> String {
    let masked = mask_secret(&credential.token);
    match credential.expires_at {
        Some(expiry) => {
            let renewal = if credential.refresh_token.is_some() {
                "renewable"
            } else {
                "not renewable"
            };
            format!(
                "{masked} (expires {}, {renewal})",
                expiry.format("%Y-%m-%dT%H:%M:%SZ")
            )
        }
        None => format!("{masked} (does not expire)"),
    }
}

fn credential_json(credential: &Credential) -> Value {
    json!({
        "token": mask_secret(&credential.token),
        "expires_at": credential.expires_at,
        "renewable": credential.refresh_token.is_some(),
        "scopes": credential.scopes,
    })
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
            for (name, profile) in &snapshot.profiles {
                revoke_quietly(client, authenticator, name, &profile.tokens).await;
            }
        }
        authenticator.clear_all().await?;
        LogoutOutcome::All
    } else {
        let target = snapshot
            .resolve(profile.as_deref())
            .context("no active profile to log out from")?
            .to_string();

        if let Some(client) = slack
            && let Some(p) = snapshot.profiles.get(&target)
        {
            revoke_quietly(client, authenticator, &target, &p.tokens).await;
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
            LogoutOutcome::All => json!({"scope": "all"}),
            LogoutOutcome::Single {
                name,
                found,
                was_active,
                new_active,
            } => json!({
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

/// Revokes every token in a profile before it is dropped locally.
///
/// The token is renewed first when it is due: revoking with a token Slack has
/// already expired fails, which would leave the installation live at Slack
/// while the only credential that could revoke it is deleted here.
async fn revoke_quietly(
    slack: &SlackClient,
    authenticator: &Authenticator,
    profile: &str,
    tokens: &TokenSet,
) {
    for (kind, _) in tokens.iter() {
        match authenticator.token_for_profile(profile, kind).await {
            Ok(token) => {
                if let Err(err) = slack.auth.revoke(token.expose_secret()).await {
                    tracing::warn!("auth.revoke failed for the {kind} token: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("could not obtain a live {kind} token to revoke: {err}");
            }
        }
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
            println!("{}", json!({"profiles": []}));
        } else {
            println!("No profiles configured. Run: slack-cli auth login");
        }
        return Ok(());
    }

    let name = snapshot
        .resolve(profile.as_deref())
        .context("no active profile selected")?
        .to_string();
    let kind = snapshot
        .profiles
        .get(&name)
        .with_context(|| format!("profile {name} not found"))?
        .tokens
        .iter()
        .next()
        .map(|(kind, _)| kind);

    // Verification renews a token that is due, so it runs before the profile
    // is read for display: the report then describes the credential the
    // profile actually holds now, and matches what every other command sees.
    let verification = match (slack, kind) {
        (Some(client), Some(kind)) => match authenticator.token_for_profile(&name, kind).await {
            Ok(token) => Some(client.auth.test(token.expose_secret()).await),
            Err(err) => Some(Err(err.into())),
        },
        _ => None,
    };

    let snapshot = authenticator.snapshot().await;
    let profile = snapshot
        .profiles
        .get(&name)
        .with_context(|| format!("profile {name} not found"))?;

    if json {
        print_status_json(&name, &snapshot.active_profile, profile, verification);
    } else {
        print_status_text(&name, profile, verification);
    }

    Ok(())
}

type Verification = Option<Result<crate::slack::SlackAuthIdentity>>;

fn print_status_json(
    name: &str,
    active: &Option<String>,
    profile: &Profile,
    verification: Verification,
) {
    let mut payload = json!({
        "profile": name,
        "active": active.as_deref() == Some(name),
        "method": profile.method.as_str(),
        "workspace": profile.workspace,
        "client_id": profile.client.as_ref().map(|c| &c.id),
        "tokens": {
            "user": profile.tokens.user.as_ref().map(credential_json),
            "bot": profile.tokens.bot.as_ref().map(credential_json),
        },
        "authorized_at": profile.authorized_at,
    });
    match verification {
        Some(Ok(identity)) => {
            payload["verified"] = serde_json::to_value(&identity).unwrap_or(Value::Null);
        }
        Some(Err(err)) => {
            payload["verified"] = json!({"error": err.to_string()});
        }
        None => {}
    }
    println!("{payload}");
}

fn print_status_text(name: &str, profile: &Profile, verification: Verification) {
    println!("profile: {name} ({})", profile.method);
    println!(
        "  workspace: {} ({})",
        profile.workspace.team_name, profile.workspace.team_id
    );
    if let Some(client) = &profile.client {
        println!("  client_id: {}", client.id);
    }
    for (kind, credential) in profile.tokens.iter() {
        println!("  {kind} token: {}", describe(credential));
        if !credential.scopes.is_empty() {
            println!("    scopes: {}", credential.scopes.join(", "));
        }
    }
    if let Some(result) = verification {
        match result {
            Ok(identity) => println!("  verified : ok ({} / {})", identity.team, identity.user),
            Err(err) => println!("  verified : failed ({err})"),
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
                json!({
                    "name": name,
                    "active": snapshot.active_profile.as_deref() == Some(name),
                    "method": profile.method.as_str(),
                    "workspace": profile.workspace,
                    "tokens": token_kinds(&profile.tokens),
                })
            })
            .collect();
        println!("{}", json!({"profiles": payload}));
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
                "{} {:<20} {:<14} {}",
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
        println!("{}", json!({"active": name}));
    } else {
        println!("✓ Active profile: {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(method: Option<AuthMethod>) -> LoginInput {
        LoginInput {
            method,
            profile: None,
            user_token: None,
            bot_token: None,
            client_id: Some("123.456".into()),
            port: DEFAULT_CALLBACK_PORT,
            no_browser: true,
        }
    }

    fn stored(client_id: Option<&str>) -> AuthConfig {
        AuthConfig {
            client_id: client_id.map(str::to_string),
            exclude_scopes: Vec::new(),
        }
    }

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

    #[test]
    fn pasted_tokens_select_the_static_method() {
        let mut given = input(None);
        given.user_token = Some(secret::new("xoxp-1"));
        assert_eq!(decide_method(&given).unwrap(), AuthMethod::Static);
    }

    #[test]
    fn an_explicit_method_always_wins() {
        let mut given = input(Some(AuthMethod::Static));
        given.user_token = None;
        assert_eq!(decide_method(&given).unwrap(), AuthMethod::Static);
    }

    #[test]
    fn the_command_line_outranks_the_stored_client_id() {
        let given = input(None).with_stored_app(&stored(Some("stored-id")));
        assert_eq!(given.client_id.as_deref(), Some("123.456"));
    }

    #[test]
    fn the_stored_client_id_fills_in_when_the_command_line_omits_it() {
        let mut given = input(None);
        given.client_id = None;
        let given = given.with_stored_app(&stored(Some("stored-id")));
        assert_eq!(given.client_id.as_deref(), Some("stored-id"));
    }

    #[test]
    fn browser_login_requires_a_client_id() {
        let err = build_client(None).unwrap_err();
        assert!(err.to_string().contains("--client-id"));
    }

    #[test]
    fn credentials_describe_their_lifetime() {
        let permanent = Credential::permanent(secret::new("xoxp-abcdefgh"), vec![]);
        assert!(describe(&permanent).contains("does not expire"));

        let rotating = Credential {
            token: secret::new("xoxe.xoxp-abcdefgh"),
            refresh_token: Some(secret::new("xoxe-refresh")),
            expires_at: Some(chrono::Utc::now()),
            scopes: vec![],
        };
        assert!(describe(&rotating).contains("renewable"));
    }
}
